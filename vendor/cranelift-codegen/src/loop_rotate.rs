//! Rotate a comparison-only loop header, retaining its original entry guard.
//! Header values join separately at the body and exit; effects are never copied.
use crate::{bitset::CompoundBitSet, dominator_tree::DominatorTree,
    flowgraph::ControlFlowGraph, ir::{Block, Function, Inst, InstructionData, Opcode, Value},
    loop_analysis::{Loop, LoopAnalysis}};
use alloc::vec::Vec;
use cranelift_entity::{EntityRef, SecondaryMap};

struct Plan {
    header: Block, body: Block, exit: Block,
    backedge: Inst, blocks: Vec<Block>, inside: CompoundBitSet, escapes: Vec<Value>,
}

impl Plan {
    fn find(f: &Function, cfg: &ControlFlowGraph, dom: &DominatorTree, loops: &LoopAnalysis, lp: Loop) -> Option<Self> {
        // Until label joins are represented, retain the existing labeled graph.
        if f.dfg.values_labels.as_ref().is_some_and(|labels| !labels.is_empty()) { return None; }
        let header = loops.loop_header(lp);
        // Egraph rematerializes cheap constants beside their comparison. They
        // are total and can be copied with the test without moving effects.
        let mut instructions = f.layout.block_insts(header)
            .skip_while(|inst| f.dfg.insts[*inst].opcode() == Opcode::Iconst);
        let comparison = instructions.next()?;
        let branch = instructions.next()?;
        if instructions.next().is_some() || !matches!(f.dfg.insts[comparison], InstructionData::IntCompare { .. }) { return None; }
        let InstructionData::Brif { arg, blocks: targets, .. } = f.dfg.insts[branch] else { return None; };
        let predicate = f.dfg.first_result(comparison);
        if f.dfg.resolve_aliases(arg) != predicate { return None; }
        let destinations = targets.map(|target| target.block(&f.dfg.value_lists));
        let body_index = destinations.iter().position(|&block| loops.is_in_loop(block, lp))?;
        let body = destinations[body_index];
        let exit = destinations[1 - body_index];
        if body == header || loops.is_in_loop(exit, lp) || !f.dfg.block_params(body).is_empty() { return None; }
        let mut incoming = cfg.pred_iter(body);
        if incoming.next()?.inst != branch || incoming.next().is_some() { return None; }
        let mut backedges = cfg.pred_iter(header).filter(|edge| loops.is_in_loop(edge.block, lp));
        let backedge = backedges.next()?.inst;
        if backedges.next().is_some() || !matches!(f.dfg.insts[backedge], InstructionData::Jump { .. }) { return None; }
        let blocks: Vec<_> = f.layout.blocks().filter(|&block| loops.is_in_loop(block, lp)).collect();
        if blocks.iter().any(|&block| loops.innermost_loop(block) != Some(lp)) { return None; }
        let mut inside = CompoundBitSet::with_capacity(f.dfg.num_blocks());
        for &block in &blocks { inside.insert(block.index()); }
        if cfg.pred_iter(exit).any(|edge| !inside.contains(edge.block.index())) { return None; }
        let mut parameters = CompoundBitSet::<usize>::with_capacity(f.dfg.num_values());
        for &value in f.dfg.block_params(header) { parameters.insert(value.index()); }
        let mut escaping = CompoundBitSet::<usize>::with_capacity(f.dfg.num_values());
        for block in f.layout.blocks() { for inst in f.layout.block_insts(block) {
            for value in f.dfg.inst_values(inst).map(|value| f.dfg.resolve_aliases(value)) {
                if value == predicate && inst != branch { return None; }
                if !inside.contains(block.index()) && parameters.contains(value.index()) {
                    if !dom.block_dominates(exit, block) { return None; }
                    escaping.insert(value.index());
                }
            }
            if inside.contains(block.index()) {
                for target in f.dfg.insts[inst].branch_destination(&f.dfg.jump_tables, &f.dfg.exception_tables) {
                    let target = target.block(&f.dfg.value_lists);
                    if !inside.contains(target.index()) && target != exit { return None; }
                }
            }
        }}
        Some(Self { header, body, exit, backedge, blocks, inside,
            escapes: escaping.iter().map(Value::new).collect() })
    }

    fn apply(self, f: &mut Function) {
        f.dfg.resolve_all_aliases();
        let parameters = f.dfg.block_params(self.header).to_vec();
        let mut body_values = SecondaryMap::<Value, Option<Value>>::with_capacity(f.dfg.num_values());
        let mut exit_values = SecondaryMap::<Value, Option<Value>>::with_capacity(f.dfg.num_values());
        for &value in &parameters {
            let ty = f.dfg.value_type(value);
            body_values[value] = Some(f.dfg.append_block_param(self.body, ty));
        }
        for &value in &self.escapes {
            let ty = f.dfg.value_type(value);
            exit_values[value] = Some(f.dfg.append_block_param(self.exit, ty));
        }
        // Rewrite uses before adding new edge arguments, which describe each
        // edge's actual source state rather than the destination's projection.
        for block in f.layout.blocks().collect::<Vec<_>>() {
            if block == self.header { continue; }
            let values = if self.inside.contains(block.index()) { &body_values } else { &exit_values };
            for inst in f.layout.block_insts(block).collect::<Vec<_>>() {
                f.dfg.map_inst_values(inst, |value| values[value].unwrap_or(value));
            }
        }
        for &block in &self.blocks { for inst in f.layout.block_insts(block).collect::<Vec<_>>() {
            let dfg = &mut f.stencil.dfg;
            for target in dfg.insts[inst].branch_destination_mut(&mut dfg.jump_tables, &mut dfg.exception_tables) {
                let destination = target.block(&dfg.value_lists);
                if destination == self.body && block == self.header {
                    for &value in &parameters { target.append_argument(value, &mut dfg.value_lists); }
                }
                if destination == self.exit {
                    for &value in &self.escapes {
                        let value = if block == self.header { value } else { body_values[value].unwrap() };
                        target.append_argument(value, &mut dfg.value_lists);
                    }
                }
            }
        }}
        let latch = f.dfg.make_block();
        f.layout.append_block(latch);
        let mut latch_values = SecondaryMap::<Value, Option<Value>>::with_capacity(f.dfg.num_values());
        for &value in &parameters {
            let ty = f.dfg.value_type(value);
            latch_values[value] = Some(f.dfg.append_block_param(latch, ty));
        }
        for inst in f.layout.block_insts(self.header).collect::<Vec<_>>() {
            let copy = f.dfg.clone_inst(inst);
            f.layout.append_inst(copy, latch);
            f.set_srcloc(copy, f.srcloc(inst));
            for (&old, &new) in f.dfg.inst_results(inst).iter().zip(f.dfg.inst_results(copy)) { latch_values[old] = Some(new); }
            f.dfg.map_inst_values(copy, |value| latch_values[value].unwrap_or(value));
        }
        f.rewrite_branch_destination(self.backedge, self.header, latch);
    }
}

pub(crate) fn run(f: &mut Function) -> bool {
    let mut changed = false;
    // Each original header is considered once; rotation must not circulate
    // comparison-only bodies indefinitely around a loop.
    let headers = {
        let cfg = ControlFlowGraph::with_function(f);
        let dom = DominatorTree::with_function(f, &cfg);
        let mut loops = LoopAnalysis::new(); loops.compute(f, &cfg, &dom);
        loops.loops().map(|lp| loops.loop_header(lp)).collect::<Vec<_>>()
    };
    for header in headers {
        let cfg = ControlFlowGraph::with_function(f);
        let dom = DominatorTree::with_function(f, &cfg);
        let mut loops = LoopAnalysis::new(); loops.compute(f, &cfg, &dom);
        if let Some(lp) = loops.is_loop_header(header)
            && let Some(plan) = Plan::find(f, &cfg, &dom, &loops, lp) {
            plan.apply(f); changed = true;
        }
    }
    changed
}
