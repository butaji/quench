//! Unswitch invariant polling effects and repeated representation choices. Only the loop is copied;
//! escaping SSA values join at their existing exits, leaving cold recovery shared.
//! Each stage selects one choice per input loop to keep code growth linear.

use crate::{bitset::CompoundBitSet, cursor::{Cursor, FuncCursor}, dominator_tree::DominatorTree,
    flowgraph::ControlFlowGraph, ir::{self, Block, Function, Inst, InstBuilder, InstructionData, Opcode, Value, ValueDef},
    loop_analysis::{Loop, LoopAnalysis}};
use alloc::vec::Vec;
use cranelift_entity::{EntityRef, SecondaryMap};

#[derive(Clone, Copy)]
struct Gate { branch: Inst, fast: Block, slow: Block, fast_truth: bool }

#[derive(Clone, Copy)]
enum Choice<'a> { Poll(&'a CompoundBitSet), Repeated }

#[derive(Clone, Copy, PartialEq, Eq)]
enum Predicate { Value(Value), BitAnd(Value, Value) }

fn gate(f: &Function, cfg: &ControlFlowGraph, used: &CompoundBitSet, inst: Inst) -> Option<(Value, Gate)> {
    let InstructionData::Brif { arg, blocks, .. } = f.dfg.insts[inst] else { return None; };
    if blocks.iter().any(|block| block.len(&f.dfg.value_lists) != 0) { return None; }
    for slow in 0..blocks.len() {
        let slow_block = blocks[slow].block(&f.dfg.value_lists);
        let fast = blocks[1 - slow].block(&f.dfg.value_lists);
        let mut incoming = cfg.pred_iter(slow_block);
        if !incoming.next().is_some_and(|edge| edge.inst == inst) || incoming.next().is_some() { continue; }
        let mut body = f.layout.block_insts(slow_block);
        let Some(load) = body.next() else { continue; };
        if f.dfg.insts[load].opcode() != Opcode::AtomicLoad { continue; }
        let Some(jump) = body.next() else { continue; };
        let InstructionData::Jump { destination, .. } = f.dfg.insts[jump] else { continue; };
        if body.next().is_some() || destination.block(&f.dfg.value_lists) != fast
            || destination.len(&f.dfg.value_lists) != 0 { continue; }
        let result = f.dfg.first_result(load);
        if used.contains(result.index()) { continue; }
        return Some((f.dfg.resolve_aliases(arg), Gate { branch: inst, fast, slow: slow_block, fast_truth: slow == 1 }));
    }
    None
}

struct Exit { block: Block, escapes: Vec<Value> }

struct Plan { header: Block, blocks: Vec<Block>, inside: CompoundBitSet, exits: Vec<Exit>,
    condition: Predicate, gates: Vec<Gate> }

impl Plan {
    fn find(f: &Function, cfg: &ControlFlowGraph, dom: &DominatorTree, loops: &LoopAnalysis, lp: Loop, choice: Choice<'_>) -> Option<Self> {
        if f.dfg.values_labels.as_ref().is_some_and(|labels| !labels.is_empty()) { return None; }
        let header = loops.loop_header(lp);
        let blocks: Vec<_> = f.layout.blocks().filter(|&block| loops.is_in_loop(block, lp)).collect();
        if blocks.iter().any(|&block| loops.innermost_loop(block) != Some(lp)) { return None; }
        let mut inside = CompoundBitSet::with_capacity(f.dfg.num_blocks());
        for block in &blocks { inside.insert(block.index()); }
        if header == f.layout.entry_block()? || cfg.pred_iter(header).all(|edge| inside.contains(edge.block.index())) { return None; }
        let invariant = |value: Value| {
            let definition = match f.dfg.value_def(value) { ValueDef::Param(block, _) => Some(block),
                ValueDef::Result(inst, _) => f.layout.inst_block(inst), _ => None };
            definition.is_some_and(|block| !inside.contains(block.index()) && dom.block_dominates(block, header))
        };
        let mut gates: Vec<_> = blocks.iter().filter_map(|&block| {
            let branch = f.layout.last_inst(block)?;
            if let Choice::Poll(used) = choice {
                return gate(f, cfg, used, branch).map(|(value, gate)| (Predicate::Value(value), gate));
            }
            let InstructionData::Brif { arg, blocks, .. } = f.dfg.insts[branch] else { return None; };
            if blocks.iter().any(|block| block.len(&f.dfg.value_lists) != 0
                || !inside.contains(block.block(&f.dfg.value_lists).index())) { return None; }
            let value = f.dfg.resolve_aliases(arg);
            let predicate = if invariant(value) { Predicate::Value(value) } else {
                // Egraph extraction rematerializes cheap bit tests at each use.
                // A bitwise AND of dominating operands is total and effect-free;
                // evaluate it once at entry without moving a load or a proof.
                let ValueDef::Result(inst, _) = f.dfg.value_def(value) else { return None; };
                let InstructionData::Binary { opcode: Opcode::Band, args } = f.dfg.insts[inst] else { return None; };
                let [mut a, mut b] = args.map(|value| f.dfg.resolve_aliases(value));
                if !invariant(a) || !invariant(b) { return None; }
                if a.index() > b.index() { core::mem::swap(&mut a, &mut b); }
                Predicate::BitAnd(a, b)
            };
            Some((predicate, Gate { branch, fast: blocks[0].block(&f.dfg.value_lists),
                slow: blocks[1].block(&f.dfg.value_lists), fast_truth: true }))
        }).collect();
        let (condition, first) = if matches!(choice, Choice::Repeated) {
            let mut counts = Vec::new();
            for &(predicate, _) in &gates {
                if let Some((_, count)) = counts.iter_mut().find(|(key, _)| *key == predicate) { *count += 1; }
                else { counts.push((predicate, 1)); }
            }
            let &(value, count) = counts.iter().max_by_key(|(_, count)| *count)?;
            // Copy a loop only when one entry choice replaces repeated tests.
            if count <= 1 { return None; }
            gates.retain(|(condition, _)| *condition == value);
            let gate = gates[0].1;
            (value, gate)
        } else { *gates.iter().find(|(value, _)| matches!(*value, Predicate::Value(value) if invariant(value)))? };
        if gates.iter().any(|(value, gate)| *value != condition || gate.fast_truth != first.fast_truth
            || !inside.contains(gate.fast.index()) || !inside.contains(gate.slow.index())) { return None; }
        let gates = gates.into_iter().map(|(_, gate)| gate).collect();
        let mut exits = Vec::new();
        let mut defined = CompoundBitSet::<usize>::with_capacity(f.dfg.num_values());
        for &block in &blocks {
            for &value in f.dfg.block_params(block) { defined.insert(value.index()); }
            for inst in f.layout.block_insts(block) {
                if f.dfg.insts[inst].opcode().is_call() || f.dfg.user_stack_map_entries(inst).is_some()
                    || matches!(f.dfg.insts[inst], InstructionData::BranchTable { .. }) { return None; }
                for &value in f.dfg.inst_results(inst) { defined.insert(value.index()); }
                for branch in f.dfg.insts[inst].branch_destination(&f.dfg.jump_tables, &f.dfg.exception_tables) {
                    let target = branch.block(&f.dfg.value_lists);
                    if !inside.contains(target.index()) {
                        if !exits.contains(&target) { exits.push(target); }
                    }
                }
            }
        }
        if exits.is_empty() { return None; }
        // Explicit edge arguments already distinguish loop copies and zero-trip
        // predecessors. Implicit uses need a dedicated, dominating exit join.
        let mut escapes: Vec<CompoundBitSet> = exits.iter()
            .map(|_| CompoundBitSet::with_capacity(f.dfg.num_values())).collect();
        for block in f.layout.blocks().filter(|block| !inside.contains(block.index())) {
            for inst in f.layout.block_insts(block) {
                for value in f.dfg.inst_values(inst).map(|value| f.dfg.resolve_aliases(value)) {
                    if !defined.contains(value.index()) { continue; }
                    let definition = match f.dfg.value_def(value) { ValueDef::Param(block, _) => block,
                        ValueDef::Result(inst, _) => f.layout.inst_block(inst)?, _ => return None };
                    let mut candidates = exits.iter().enumerate().filter(|(_, exit)|
                        dom.block_dominates(**exit, block) && dom.block_dominates(definition, **exit)
                            && cfg.pred_iter(**exit).all(|edge| inside.contains(edge.block.index())));
                    let (index, _) = candidates.next()?;
                    if candidates.next().is_some() { return None; }
                    escapes[index].insert(value.index());
                }
            }
        }
        let exits = exits.into_iter().zip(escapes).map(|(block, values)|
            Exit { block, escapes: values.iter().map(Value::new).collect() }).collect();
        Some(Self { header, blocks, inside, exits, condition, gates })
    }

    fn apply(self, f: &mut Function, cfg: &ControlFlowGraph, dom: &DominatorTree) {
        f.dfg.resolve_all_aliases();
        for exit in &self.exits {
            if exit.escapes.is_empty() { continue; }
            let mut joins = SecondaryMap::<Value, Option<Value>>::with_capacity(f.dfg.num_values());
            for &value in &exit.escapes {
                let ty = f.dfg.value_type(value);
                joins[value] = Some(f.dfg.append_block_param(exit.block, ty));
            }
            let outside: Vec<_> = f.layout.blocks().filter(|block|
                !self.inside.contains(block.index()) && dom.block_dominates(exit.block, *block)).collect();
            for block in outside { for inst in f.layout.block_insts(block).collect::<Vec<_>>() {
                f.dfg.map_inst_values(inst, |value| joins[value].unwrap_or(value));
            }}
        }
        for &block in &self.blocks { for inst in f.layout.block_insts(block).collect::<Vec<_>>() {
            let dfg = &mut f.stencil.dfg;
            for branch in dfg.insts[inst].branch_destination_mut(&mut dfg.jump_tables, &mut dfg.exception_tables) {
                if let Some(exit) = self.exits.iter().find(|exit| exit.block == branch.block(&dfg.value_lists)) {
                    for &value in &exit.escapes { branch.append_argument(value, &mut dfg.value_lists); }
                }
            }
        }}
        let mut blocks = SecondaryMap::<Block, Option<Block>>::with_capacity(f.dfg.num_blocks());
        let mut values = SecondaryMap::<Value, Option<Value>>::with_capacity(f.dfg.num_values());
        let mut instructions = SecondaryMap::<Inst, Option<Inst>>::with_capacity(f.dfg.num_insts());
        for &block in &self.blocks {
            let copy = f.dfg.make_block(); f.layout.append_block(copy); blocks[block] = Some(copy);
            if f.layout.is_cold(block) { f.layout.set_cold(copy); }
            for value in f.dfg.block_params(block).to_vec() {
                let ty = f.dfg.value_type(value); values[value] = Some(f.dfg.append_block_param(copy, ty));
            }
            for inst in f.layout.block_insts(block).collect::<Vec<_>>() {
                let copy_inst = f.dfg.clone_inst(inst); f.layout.append_inst(copy_inst, copy);
                if f.private_stores.contains(&inst) { f.private_stores.push(copy_inst); }
                f.debug_tags.clone_tags(inst, copy_inst);
                let location = f.srcloc(inst); f.set_srcloc(copy_inst, location); instructions[inst] = Some(copy_inst);
                for (&old, &new) in f.dfg.inst_results(inst).iter().zip(f.dfg.inst_results(copy_inst)) { values[old] = Some(new); }
            }
        }
        for &block in &self.blocks {
            let copy = blocks[block].unwrap();
            for inst in f.layout.block_insts(copy).collect::<Vec<_>>() {
                f.dfg.map_inst_values(inst, |value| values[value].unwrap_or(value));
                let dfg = &mut f.stencil.dfg;
                for branch in dfg.insts[inst].branch_destination_mut(&mut dfg.jump_tables, &mut dfg.exception_tables) {
                    if let Some(target) = blocks[branch.block(&dfg.value_lists)] { branch.set_block(target, &mut dfg.value_lists); }
                }
            }
        }
        let gateway = f.dfg.make_block(); f.layout.insert_block(gateway, self.header);
        let args: Vec<ir::BlockArg> = f.dfg.block_params(self.header).to_vec().into_iter()
            .map(|value| { let ty = f.dfg.value_type(value); f.dfg.append_block_param(gateway, ty).into() }).collect();
        for edge in cfg.pred_iter(self.header).filter(|edge| !self.inside.contains(edge.block.index())) {
            f.rewrite_branch_destination(edge.inst, self.header, gateway);
        }
        let mut cursor = FuncCursor::new(f); cursor.goto_bottom(gateway);
        let original = self.header; let copy = blocks[self.header].unwrap();
        let (yes, no) = if self.gates[0].fast_truth { (original, copy) } else { (copy, original) };
        let condition = match self.condition {
            Predicate::Value(value) => value,
            Predicate::BitAnd(a, b) => cursor.ins().band(a, b),
        };
        cursor.ins().brif(condition, yes, &args, no, &args);
        for gate in self.gates {
            cursor.func.replace(gate.branch).jump(gate.fast, &[]);
            cursor.func.replace(instructions[gate.branch].unwrap()).jump(blocks[gate.slow].unwrap(), &[]);
        }
    }
}

pub(crate) fn run(f: &mut Function) -> bool {
    let mut changed = false;
    loop {
        let cfg = ControlFlowGraph::with_function(f);
        let dom = DominatorTree::with_function(f, &cfg);
        let mut loops = LoopAnalysis::new(); loops.compute(f, &cfg, &dom);
        let mut used = CompoundBitSet::with_capacity(f.dfg.num_values());
        for block in f.layout.blocks() { for inst in f.layout.block_insts(block) {
            for value in f.dfg.inst_values(inst) { used.insert(f.dfg.resolve_aliases(value).index()); }
        }}
        let Some(plan) = loops.loops().find_map(|lp| Plan::find(f, &cfg, &dom, &loops, lp, Choice::Poll(&used))) else { break; };
        plan.apply(f, &cfg, &dom); changed = true;
    }
    for specialization in core::mem::take(&mut f.effect_specializations) {
        for (inst, truth) in specialization.gates {
            if f.layout.inst_block(inst).is_none() { continue; }
            if let InstructionData::Brif { blocks, .. } = f.dfg.insts[inst] {
                let selected = blocks[usize::from(!truth)];
                let target = selected.block(&f.dfg.value_lists);
                let args: Vec<_> = selected.args(&f.dfg.value_lists).collect();
                f.replace(inst).jump(target, &args); changed = true;
            }
        }
        let proof = specialization.proof;
        if f.layout.inst_block(proof).is_some() && f.debug_tags.get(proof).is_empty() {
            let results = f.dfg.inst_results(proof);
            let used = f.layout.blocks().flat_map(|block| f.layout.block_insts(block))
                .any(|inst| f.dfg.inst_values(inst).any(|value| results.contains(&f.dfg.resolve_aliases(value))));
            if !used { f.layout.remove_inst(proof); changed = true; }
        }
    }
    // Visit only the input headers: recursively specializing copies would form
    // a Cartesian product of independent choices. One choice per input loop
    // bounds this stage's growth to one additional copy of each loop.
    let cfg = ControlFlowGraph::with_function(f);
    let dom = DominatorTree::with_function(f, &cfg);
    let mut loops = LoopAnalysis::new(); loops.compute(f, &cfg, &dom);
    let headers: Vec<_> = loops.loops().map(|lp| loops.loop_header(lp)).collect();
    for header in headers {
        let cfg = ControlFlowGraph::with_function(f);
        let dom = DominatorTree::with_function(f, &cfg);
        let mut loops = LoopAnalysis::new(); loops.compute(f, &cfg, &dom);
        let Some(lp) = loops.loops().find(|lp| loops.loop_header(*lp) == header) else { continue; };
        if let Some(plan) = Plan::find(f, &cfg, &dom, &loops, lp, Choice::Repeated) {
            plan.apply(f, &cfg, &dom); changed = true;
        }
    }
    changed
}

/// Commit loop-carried memory mirrors at the sole exit when the loop has no
/// observer, trap, or poll. Polling versions deliberately remain write-through:
/// their fault landing can bypass generated exit blocks.
pub(crate) fn sink_stores(f: &mut Function) -> bool {
    if f.private_stores.is_empty() || f.dfg.values_labels.as_ref().is_some_and(|labels| !labels.is_empty()) { return false; }
    let cfg = ControlFlowGraph::with_function(f);
    let dom = DominatorTree::with_function(f, &cfg);
    let mut loops = LoopAnalysis::new(); loops.compute(f, &cfg, &dom);
    let mut changed = false;
    let mut private_stores = CompoundBitSet::<usize>::with_capacity(f.dfg.num_insts());
    for &inst in &f.private_stores { private_stores.insert(inst.index()); }
    for lp in loops.loops() {
        let header = loops.loop_header(lp);
        let blocks: Vec<_> = f.layout.blocks().filter(|&block| loops.is_in_loop(block, lp)).collect();
        if blocks.iter().any(|&block| loops.innermost_loop(block) != Some(lp)) { continue; }
        let mut stores = Vec::new();
        let mut exits = Vec::new();
        let mut latches = Vec::new();
        let mut eligible = true;
        for &block in &blocks { for inst in f.layout.block_insts(block) {
            let data = &f.dfg.insts[inst]; let op = data.opcode();
            if f.dfg.user_stack_map_entries(inst).is_some() || !f.debug_tags.get(inst).is_empty() {
                eligible = false; break;
            }
            if let InstructionData::Store { args, flags, .. } = *data {
                let address = f.dfg.resolve_aliases(args[1]);
                let owner = match f.dfg.value_def(address) {
                    ValueDef::Param(block, _) => Some(block),
                    ValueDef::Result(inst, _) => f.layout.inst_block(inst), _ => None,
                };
                if !private_stores.contains(inst.index()) || !f.dfg.mem_flags[flags].notrap() || !owner.is_some_and(|owner|
                    !loops.is_in_loop(owner, lp) && dom.block_dominates(owner, header)) {
                    eligible = false; break;
                }
                stores.push(inst);
            } else if matches!(op, Opcode::Jump | Opcode::Brif) {
                for (index, edge) in data.branch_destination(&f.dfg.jump_tables, &f.dfg.exception_tables).iter().enumerate() {
                    let target = edge.block(&f.dfg.value_lists);
                    if !loops.is_in_loop(target, lp) { exits.push((inst, index)); }
                    else if target == header { latches.push(inst); }
                }
            } else if op.can_load() || op.can_store() || op.can_trap() || op.is_call()
                || op.other_side_effects() || op.is_terminator() {
                eligible = false; break;
            }
        }}
        if !eligible || stores.is_empty() || exits.len() != 1 || latches.is_empty() { continue; }
        let (exit, edge_index) = exits[0];
        if stores.iter().any(|&store| !dom.dominates(store, exit, &f.layout)
            || latches.iter().any(|&latch| !dom.dominates(store, latch, &f.layout))) { continue; }
        // Sink pure encoding trees used only by the committed stores as well.
        // Loop-carried values and values used by recovery retain their homes.
        let mut uses = alloc::vec![0_usize; f.dfg.num_values()];
        for block in f.layout.blocks() { for inst in f.layout.block_insts(block) {
            for value in f.dfg.inst_values(inst) { uses[f.dfg.resolve_aliases(value).index()] += 1; }
        }}
        let mut selected = CompoundBitSet::<usize>::with_capacity(f.dfg.num_insts());
        for &store in &stores { selected.insert(store.index()); }
        let mut next = 0;
        while next < stores.len() {
            let inst = stores[next]; next += 1;
            for value in f.dfg.inst_values(inst) {
                let value = f.dfg.resolve_aliases(value);
                uses[value.index()] -= 1;
                let ValueDef::Result(definition, _) = f.dfg.value_def(value) else { continue; };
                if selected.contains(definition.index()) || !f.layout.inst_block(definition)
                    .is_some_and(|block| loops.is_in_loop(block, lp)) { continue; }
                let op = f.dfg.insts[definition].opcode();
                if op.is_branch() || op.is_terminator() || op.can_store()
                    || !f.dfg.inst_results(definition).iter().all(|value| uses[value.index()] == 0) { continue; }
                selected.insert(definition.index()); stores.push(definition);
            }
        }
        // All stores dominate the same exit, so dominance totally orders them.
        // Keep that order even when their invariant addresses may alias.
        stores.sort_by(|&a, &b| if a == b { core::cmp::Ordering::Equal }
            else if dom.dominates(a, b, &f.layout) { core::cmp::Ordering::Less }
            else { core::cmp::Ordering::Greater });
        let destination = f.dfg.insts[exit].branch_destination(&f.dfg.jump_tables, &f.dfg.exception_tables)[edge_index];
        let target = destination.block(&f.dfg.value_lists);
        let args: Vec<_> = destination.args(&f.dfg.value_lists).collect();
        let commit = f.dfg.make_block(); f.layout.append_block(commit);
        let mut cursor = FuncCursor::new(f); cursor.goto_bottom(commit);
        let jump = cursor.ins().jump(target, &args);
        for store in stores { cursor.func.layout.remove_inst(store); cursor.func.layout.insert_inst(store, jump); }
        let dfg = &mut cursor.func.dfg;
        dfg.insts[exit].branch_destination_mut(&mut dfg.jump_tables,
            &mut dfg.exception_tables)[edge_index] = ir::BlockCall::new(commit, [], &mut dfg.value_lists);
        changed = true;
    }
    changed
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ir::{AbiParam, MemFlagsData, Signature, UserFuncName, types, condcodes::IntCC};

    #[test]
    fn store_commit_requires_an_unobserved_complete_iteration() {
        #[derive(Clone, Copy, PartialEq, Eq)]
        enum Barrier { None, Poll, Read, EarlyExit, UnprovedStore }
        for barrier in [Barrier::None, Barrier::Poll, Barrier::Read, Barrier::EarlyExit, Barrier::UnprovedStore] {
            let mut f = fixture(false, false, false);
            let pointer = f.dfg.block_params(f.layout.entry_block().unwrap())[1];
            let poll = f.layout.blocks().flat_map(|block| f.layout.block_insts(block))
                .find(|&inst| f.dfg.insts[inst].opcode() == Opcode::AtomicLoad).unwrap();
            if barrier != Barrier::Poll { f.layout.remove_inst(poll); }
            let body = f.layout.blocks().find(|&block| f.layout.first_inst(block)
                .is_some_and(|inst| f.dfg.insts[inst].opcode() == Opcode::Iadd)).unwrap();
            let exit = f.layout.last_inst(body).unwrap();
            let value = f.dfg.first_result(f.layout.first_inst(body).unwrap());
            let mut cursor = FuncCursor::new(&mut f); cursor.goto_inst(exit);
            let first = cursor.ins().store(MemFlagsData::trusted(), value, pointer, 0);
            if barrier == Barrier::Read { cursor.ins().load(types::I32, MemFlagsData::trusted(), pointer, 0); }
            let second = cursor.ins().store(MemFlagsData::trusted(), value, pointer, 0);
            if barrier != Barrier::UnprovedStore { cursor.func.private_stores.extend([first, second]); }
            if barrier == Barrier::EarlyExit {
                // An alternate exit before the stores must not commit them.
                let header = f.layout.blocks().find(|&block| f.dfg.block_params(block).len() == 2).unwrap();
                let branch = f.layout.last_inst(header).unwrap();
                let early = f.dfg.make_block(); f.layout.append_block(early);
                let count = f.dfg.block_params(header)[0];
                let mut cursor = FuncCursor::new(&mut f); cursor.goto_bottom(early); cursor.ins().return_(&[count]);
                let dfg = &mut f.dfg;
                dfg.insts[branch].branch_destination_mut(&mut dfg.jump_tables,
                    &mut dfg.exception_tables)[0] = ir::BlockCall::new(early, [], &mut dfg.value_lists);
            }
            let flags = crate::settings::Flags::new(crate::settings::builder());
            crate::verifier::verify_function(&f, &flags).unwrap();
            let original = f.clone();
            assert_eq!(sink_stores(&mut f), barrier == Barrier::None);
            crate::verifier::verify_function(&f, &flags).unwrap_or_else(|error| panic!("{error}\n{f}"));
            if barrier == Barrier::None {
                let commit = f.layout.inst_block(first).unwrap();
                assert_ne!(commit, body);
                assert_eq!(f.layout.inst_block(second), Some(commit));
                assert_eq!(f.layout.next_inst(first), Some(second));
            } else { assert_eq!(f, original); }
        }
    }

    fn fixture(invariant: bool, observed: bool, reverse: bool) -> Function {
        let mut signature = Signature::new(crate::isa::CallConv::SystemV);
        signature.params.extend([AbiParam::new(types::I8), AbiParam::new(types::I64), AbiParam::new(types::I32)]);
        signature.returns.push(AbiParam::new(types::I32));
        let mut f = Function::with_name_signature(UserFuncName::testcase("poll_choice"), signature);
        let entry = f.dfg.make_block(); let pre = f.dfg.make_block(); let other = f.dfg.make_block();
        let header = f.dfg.make_block(); let poll = f.dfg.make_block(); let body = f.dfg.make_block(); let exit = f.dfg.make_block();
        let private = f.dfg.append_block_param(entry, types::I8);
        let pointer = f.dfg.append_block_param(entry, types::I64);
        let bound = f.dfg.append_block_param(entry, types::I32);
        let count = f.dfg.append_block_param(header, types::I32);
        let sum = f.dfg.append_block_param(header, types::I32);
        let mut cursor = FuncCursor::new(&mut f); cursor.insert_block(entry);
        let zero = cursor.ins().iconst(types::I32, 0); let one = cursor.ins().iconst(types::I32, 1);
        cursor.ins().brif(private, pre, &[], other, &[]);
        cursor.insert_block(pre); cursor.ins().jump(header, &[zero.into(), one.into()]);
        cursor.insert_block(other); cursor.ins().jump(header, &[zero.into(), zero.into()]);
        cursor.insert_block(header);
        let condition = if invariant { private } else { cursor.ins().icmp_imm_s(IntCC::Equal, count, 0) };
        if reverse { cursor.ins().brif(condition, poll, &[], body, &[]); }
        else { cursor.ins().brif(condition, body, &[], poll, &[]); }
        cursor.insert_block(poll); let loaded = cursor.ins().atomic_load(types::I8, MemFlagsData::trusted(), pointer);
        if observed { cursor.ins().store(MemFlagsData::trusted(), loaded, pointer, 1); }
        cursor.ins().jump(body, &[]);
        cursor.insert_block(body); let next = cursor.ins().iadd(count, one); let added = cursor.ins().iadd(sum, next);
        let more = cursor.ins().icmp(IntCC::SignedLessThan, next, bound);
        cursor.ins().brif(more, header, &[next.into(), added.into()], exit, &[]);
        cursor.insert_block(exit); let result = cursor.ins().bxor(next, added); cursor.ins().return_(&[result]);
        f
    }

    #[test]
    fn repeated_invariant_stores_share_one_entry_choice() {
        for rematerialized in [false, true] { for independent in [false, true] { for varying in [false, true] {
        let mut f = fixture(false, false, false);
        let entry = f.layout.entry_block().unwrap();
        let private = f.dfg.block_params(entry)[0];
        let pointer = f.dfg.block_params(entry)[1];
        let bound = f.dfg.block_params(entry)[2];
        let terminator = f.layout.last_inst(entry).unwrap();
        let mut cursor = FuncCursor::new(&mut f); cursor.goto_inst(terminator);
        let condition = cursor.ins().uextend(types::I32, private);
        let mask = cursor.ins().iconst(types::I32, 1);
        let header = f.layout.blocks().find(|block| f.dfg.block_params(*block).len() == 2).unwrap();
        let count = f.dfg.block_params(header)[0];
        let sum = f.dfg.block_params(header)[1];
        let body = f.layout.blocks().find(|block| f.layout.first_inst(*block)
            .is_some_and(|inst| f.dfg.insts[inst].opcode() == Opcode::Iadd)).unwrap();
        let first = f.layout.first_inst(body).unwrap();
        let rest = f.dfg.make_block(); f.layout.split_block(rest, first);
        let mut current = body;
        let fields = [(count, sum, 0), (sum, count, 4)];
        let stores = fields.len() * if independent { 2 } else { 1 };
        for (index, (yes_value, no_value, offset)) in fields.into_iter().cycle().take(stores).enumerate() {
            let yes = f.dfg.make_block(); f.layout.append_block(yes);
            let no = f.dfg.make_block(); f.layout.append_block(no);
            let join = f.dfg.make_block(); f.layout.append_block(join);
            let mut cursor = FuncCursor::new(&mut f); cursor.goto_bottom(current);
            let selected = match (varying, index < fields.len()) {
                (false, true) => condition, (false, false) => bound,
                (true, true) => count, (true, false) => sum,
            };
            let selected = if !rematerialized { selected }
                else if index % 2 == 0 { cursor.ins().band(selected, mask) }
                else { cursor.ins().band(mask, selected) };
            cursor.ins().brif(selected, yes, &[], no, &[]);
            for (block, value) in [(yes, yes_value), (no, no_value)] {
                cursor.goto_bottom(block);
                cursor.ins().store(MemFlagsData::trusted(), value, pointer, offset);
                cursor.ins().jump(join, &[]);
            }
            current = join;
        }
        let mut cursor = FuncCursor::new(&mut f); cursor.goto_bottom(current); cursor.ins().jump(rest, &[]);
        let flags = crate::settings::Flags::new(crate::settings::builder());
        crate::verifier::verify_function(&f, &flags).unwrap();
        if varying {
            let original = f.clone(); assert!(!run(&mut f)); assert_eq!(f, original); continue;
        }
        assert!(run(&mut f));
        crate::verifier::verify_function(&f, &flags).unwrap_or_else(|error| panic!("{error}\n{f}"));
        let mut cfg = ControlFlowGraph::with_function(&f); let dom = DominatorTree::with_function(&f, &cfg);
        crate::unreachable_code::eliminate_unreachable_code(&mut f, &mut cfg, |block| dom.is_reachable(block));
        let cfg = ControlFlowGraph::with_function(&f); let dom = DominatorTree::with_function(&f, &cfg);
        let mut loops = LoopAnalysis::new(); loops.compute(&f, &cfg, &dom);
        assert_eq!(loops.loops().count(), 2);
        for lp in loops.loops() {
            let insts: Vec<_> = f.layout.blocks().filter(|&block| loops.is_in_loop(block, lp))
                .flat_map(|block| f.layout.block_insts(block)).collect();
            // The other family, when present, retains both arms and tests.
            let remaining_stores = fields.len() * if independent { 3 } else { 1 };
            assert_eq!(insts.iter().filter(|&&inst| f.dfg.insts[inst].opcode() == Opcode::Store).count(), remaining_stores);
            let dynamic_tests = 2; // Poll condition and loop bound in the fixture.
            let remaining_tests = dynamic_tests + if independent { fields.len() } else { 0 };
            assert_eq!(insts.iter().filter(|&&inst| f.dfg.insts[inst].opcode() == Opcode::Brif).count(), remaining_tests);
        }
        if !independent { assert!(!run(&mut f)); }
        }}}
    }

    #[test]
    fn invariant_poll_versions_join_escaping_values_and_distinct_entry_arguments() {
        let flags = crate::settings::Flags::new(crate::settings::builder());
        for reverse in [false, true] {
            let mut f = fixture(true, false, reverse);
            crate::verifier::verify_function(&f, &flags).unwrap();
            assert!(run(&mut f));
            crate::verifier::verify_function(&f, &flags).unwrap_or_else(|error| panic!("{error}\n{f}"));
            assert!(!run(&mut f), "both versions are already specialized");
            let mut cfg = ControlFlowGraph::with_function(&f); let dom = DominatorTree::with_function(&f, &cfg);
            crate::unreachable_code::eliminate_unreachable_code(&mut f, &mut cfg, |block| dom.is_reachable(block));
            let cfg = ControlFlowGraph::with_function(&f); let dom = DominatorTree::with_function(&f, &cfg);
            let mut loops = LoopAnalysis::new(); loops.compute(&f, &cfg, &dom);
            let mut polls = Vec::new();
            for lp in loops.loops() {
                polls.push(f.layout.blocks().filter(|&block| loops.is_in_loop(block, lp))
                    .flat_map(|block| f.layout.block_insts(block)).filter(|&inst| f.dfg.insts[inst].opcode() == Opcode::AtomicLoad).count());
            }
            polls.sort_unstable(); assert_eq!(polls, [0, 1]);
        }
    }

    fn add_early_exit(f: &mut Function, merge: bool) {
        let header = f.layout.blocks().find(|block| f.dfg.block_params(*block).len() == 2).unwrap();
        let count = f.dfg.block_params(header)[0];
        let sum = f.dfg.block_params(header)[1];
        let body = f.layout.blocks().find(|block| f.layout.first_inst(*block)
            .is_some_and(|inst| f.dfg.insts[inst].opcode() == Opcode::Iadd)).unwrap();
        let first = f.layout.first_inst(body).unwrap();
        let rest = f.dfg.make_block(); f.layout.split_block(rest, first);
        let early = f.dfg.make_block(); f.layout.append_block(early);
        let original_exit = f.layout.blocks().find(|block| f.layout.last_inst(*block)
            .is_some_and(|inst| f.dfg.insts[inst].opcode() == Opcode::Return)).unwrap();
        let mut cursor = FuncCursor::new(f); cursor.goto_bottom(body);
        let stop = cursor.ins().icmp_imm_s(IntCC::Equal, count, 3);
        cursor.ins().brif(stop, early, &[], rest, &[]);
        cursor.goto_bottom(early);
        let value = cursor.ins().bxor(count, sum); cursor.ins().return_(&[value]);
        if merge {
            let joined = cursor.func.dfg.make_block(); cursor.func.layout.append_block(joined);
            for exit in [early, original_exit] {
                let ret = cursor.func.layout.last_inst(exit).unwrap();
                cursor.func.replace(ret).jump(joined, &[]);
            }
            cursor.goto_bottom(joined); cursor.ins().return_(&[count]);
        }
    }

    #[test]
    fn distinct_recovery_exits_join_their_own_typed_values() {
        let mut f = fixture(true, false, false); add_early_exit(&mut f, false);
        let header = f.layout.blocks().find(|block| f.dfg.block_params(*block).len() == 2).unwrap();
        let first = f.layout.first_inst(header).unwrap();
        let mut cursor = FuncCursor::new(&mut f); cursor.goto_inst(first);
        let point = cursor.ins().sequence_point();
        let tags = [ir::DebugTag::User(0)];
        cursor.func.debug_tags.set(point, tags.clone());
        let flags = crate::settings::Flags::new(crate::settings::builder());
        crate::verifier::verify_function(&f, &flags).unwrap();
        assert!(run(&mut f));
        crate::verifier::verify_function(&f, &flags).unwrap_or_else(|error| panic!("{error}\n{f}"));
        let exits: Vec<_> = f.layout.blocks().filter(|block| f.layout.last_inst(*block)
            .is_some_and(|inst| f.dfg.insts[inst].opcode() == Opcode::Return)).collect();
        assert_eq!(exits.len(), 2);
        let points: Vec<_> = f.layout.blocks().flat_map(|block| f.layout.block_insts(block))
            .filter(|inst| f.dfg.insts[*inst].opcode() == Opcode::SequencePoint).collect();
        assert_eq!(points.len(), 2);
        for point in points { assert_eq!(f.debug_tags.get(point), tags); }
        for exit in exits {
            assert_eq!(f.dfg.block_params(exit).len(), 2);
            assert!(f.dfg.block_params(exit).iter().all(|value| f.dfg.value_type(*value) == types::I32));
        }
        assert!(!run(&mut f));
    }

    #[test]
    fn merging_implicit_uses_need_an_explicit_ssa_join() {
        let mut f = fixture(true, false, false); add_early_exit(&mut f, true);
        crate::verifier::verify_function(&f, &crate::settings::Flags::new(crate::settings::builder())).unwrap();
        let original = f.clone();
        assert!(!run(&mut f)); assert_eq!(f, original);
    }

    #[test]
    fn explicit_exit_arguments_preserve_zero_trip_predecessors() {
        let mut f = fixture(true, false, false); add_early_exit(&mut f, false);
        let exit = f.layout.blocks().find(|block| f.layout.last_inst(*block)
            .is_some_and(|inst| f.dfg.insts[inst].opcode() == Opcode::Return)).unwrap();
        let result = f.layout.first_inst(exit).unwrap();
        let inputs = f.dfg.inst_args(result).to_vec();
        let params: Vec<_> = inputs.iter().map(|value| {
            let ty = f.dfg.value_type(*value); f.dfg.append_block_param(exit, ty)
        }).collect();
        f.dfg.map_inst_values(result, |value| params[inputs.iter().position(|input| *input == value).unwrap()]);
        let cfg = ControlFlowGraph::with_function(&f);
        for edge in cfg.pred_iter(exit) {
            let dfg = &mut f.stencil.dfg;
            for branch in dfg.insts[edge.inst].branch_destination_mut(&mut dfg.jump_tables, &mut dfg.exception_tables) {
                if branch.block(&dfg.value_lists) == exit {
                    for &value in &inputs { branch.append_argument(value, &mut dfg.value_lists); }
                }
            }
        }
        let entry = f.layout.entry_block().unwrap();
        let bound = f.dfg.block_params(entry)[2];
        let entry_branch = f.layout.last_inst(entry).unwrap();
        let other = f.dfg.insts[entry_branch].branch_destination(&f.dfg.jump_tables, &f.dfg.exception_tables)[1]
            .block(&f.dfg.value_lists);
        let zero_trip = f.layout.last_inst(other).unwrap();
        f.replace(zero_trip).jump(exit, &[bound.into(), bound.into()]);
        let original_zero_trip = f.dfg.insts[zero_trip];
        let flags = crate::settings::Flags::new(crate::settings::builder());
        crate::verifier::verify_function(&f, &flags).unwrap();
        assert!(run(&mut f));
        crate::verifier::verify_function(&f, &flags).unwrap_or_else(|error| panic!("{error}\n{f}"));
        assert_eq!(f.dfg.block_params(exit).len(), params.len());
        assert_eq!(f.dfg.insts[zero_trip], original_zero_trip);
    }

    #[test]
    fn changing_conditions_and_observed_atomic_values_are_not_polling_contracts() {
        for (invariant, observed) in [(false, false), (true, true)] {
            let mut f = fixture(invariant, observed, false); let original = f.clone();
            assert!(!run(&mut f)); assert_eq!(f, original);
        }
    }

    #[test]
    fn independent_poll_choices_do_not_multiply_loop_versions() {
        let mut f = fixture(true, false, false);
        let entry = f.layout.entry_block().unwrap();
        let pointer = f.dfg.block_params(entry)[1]; let bound = f.dfg.block_params(entry)[2];
        let old_poll = f.layout.blocks().find(|&block| f.layout.first_inst(block)
            .is_some_and(|inst| f.dfg.insts[inst].opcode() == Opcode::AtomicLoad)).unwrap();
        let InstructionData::Jump { destination, .. } = f.dfg.insts[f.layout.last_inst(old_poll).unwrap()] else { unreachable!() };
        let body = destination.block(&f.dfg.value_lists); let first = f.layout.first_inst(body).unwrap();
        let rest = f.dfg.make_block(); f.layout.split_block(rest, first);
        let poll = f.dfg.make_block(); f.layout.append_block(poll);
        let last = f.layout.last_inst(entry).unwrap(); let mut cursor = FuncCursor::new(&mut f); cursor.goto_inst(last);
        let condition = cursor.ins().icmp_imm_s(IntCC::SignedGreaterThan, bound, 17);
        cursor.goto_bottom(body); cursor.ins().brif(condition, rest, &[], poll, &[]);
        cursor.goto_bottom(poll); cursor.ins().atomic_load(types::I8, MemFlagsData::trusted(), pointer); cursor.ins().jump(rest, &[]);
        let original = f.clone(); assert!(!run(&mut f)); assert_eq!(f, original);
    }

    fn annotate_fallback(f: &mut Function) -> (Inst, Inst) {
        let entry = f.layout.entry_block().unwrap();
        let gate = f.layout.blocks().find_map(|block| {
            let inst = f.layout.last_inst(block)?;
            let InstructionData::Brif { blocks, .. } = f.dfg.insts[inst] else { return None; };
            blocks.iter().any(|target| f.layout.first_inst(target.block(&f.dfg.value_lists))
                .is_some_and(|first| f.dfg.insts[first].opcode() == Opcode::AtomicLoad)).then_some(inst)
        }).unwrap();
        let bound = f.dfg.block_params(entry)[2]; let last = f.layout.last_inst(entry).unwrap();
        let mut cursor = FuncCursor::new(f); cursor.goto_inst(last);
        let query = cursor.ins().icmp_imm_s(IntCC::Equal, bound, 0);
        let ValueDef::Result(proof, _) = cursor.func.dfg.value_def(query) else { unreachable!() };
        cursor.func.effect_specializations.push(ir::function::EffectSpecialization { proof, gates: alloc::vec![(gate, false)] });
        (proof, gate)
    }

    #[test]
    fn nonversionable_effect_contract_retains_poll_and_removes_unused_query() {
        let mut f = fixture(false, false, false); let (proof, gate) = annotate_fallback(&mut f);
        assert!(run(&mut f));
        assert!(matches!(f.dfg.insts[gate], InstructionData::Jump { .. }));
        assert!(f.layout.inst_block(proof).is_none());
        assert!(f.effect_specializations.is_empty());
        crate::verifier::verify_function(&f, &crate::settings::Flags::new(crate::settings::builder())).unwrap();
    }

    #[test]
    fn inlining_remaps_effect_contracts_before_fallback() {
        use crate::inline::{Inline, InlineCommand};
        struct Callee(Function);
        impl Inline for Callee {
            fn inline(&mut self, _: &Function, _: Inst, _: Opcode, _: ir::FuncRef, _: &[Value]) -> InlineCommand<'_> {
                InlineCommand::Inline { callee: alloc::borrow::Cow::Borrowed(&self.0), visit_callee: false }
            }
        }
        let mut callee = fixture(false, false, false); annotate_fallback(&mut callee);
        let mut caller = Function::with_name_signature(UserFuncName::testcase("effect_caller"), callee.signature.clone());
        let entry = caller.dfg.make_block();
        let args: Vec<_> = callee.signature.params.iter().map(|param| caller.dfg.append_block_param(entry, param.value_type)).collect();
        let signature = caller.import_signature(callee.signature.clone());
        let name = caller.declare_imported_user_function(ir::UserExternalName::new(1, 0));
        let target = caller.import_function(ir::ExtFuncData { name: ir::ExternalName::user(name), signature, colocated: false, patchable: false });
        let mut cursor = FuncCursor::new(&mut caller); cursor.insert_block(entry);
        let call = cursor.ins().call(target, &args); let result = cursor.func.dfg.first_result(call); cursor.ins().return_(&[result]);
        let mut context = crate::Context::for_function(caller); assert!(context.inline(Callee(callee)).unwrap());
        assert_eq!(context.func.effect_specializations.len(), 1);
        let proof = context.func.effect_specializations[0].proof;
        let gate = context.func.effect_specializations[0].gates[0].0;
        assert!(run(&mut context.func));
        assert!(context.func.layout.inst_block(proof).is_none());
        assert!(matches!(context.func.dfg.insts[gate], InstructionData::Jump { .. }));
        crate::verifier::verify_function(&context.func, &crate::settings::Flags::new(crate::settings::builder())).unwrap();
    }
}
