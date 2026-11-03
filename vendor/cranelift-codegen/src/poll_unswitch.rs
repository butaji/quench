//! Unswitch an invariant conditional polling effect. Only the loop is copied;
//! escaping SSA values join at its existing exit, leaving cold recovery shared.
//! One uniform effect choice per loop keeps code growth linear.

use crate::{bitset::CompoundBitSet, cursor::{Cursor, FuncCursor}, dominator_tree::DominatorTree,
    flowgraph::ControlFlowGraph, ir::{self, Block, Function, Inst, InstBuilder, InstructionData, Opcode, Value, ValueDef},
    loop_analysis::{Loop, LoopAnalysis}};
use alloc::vec::Vec;
use cranelift_entity::{EntityRef, SecondaryMap};

#[derive(Clone, Copy)]
struct Gate { branch: Inst, fast: Block, slow: Block, fast_truth: bool }

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

struct Plan { header: Block, blocks: Vec<Block>, inside: CompoundBitSet, exit: Block,
    condition: Value, gates: Vec<Gate>, escapes: Vec<Value> }

impl Plan {
    fn find(f: &Function, cfg: &ControlFlowGraph, dom: &DominatorTree, loops: &LoopAnalysis, used: &CompoundBitSet, lp: Loop) -> Option<Self> {
        if f.dfg.values_labels.as_ref().is_some_and(|labels| !labels.is_empty()) { return None; }
        let header = loops.loop_header(lp);
        let blocks: Vec<_> = f.layout.blocks().filter(|&block| loops.is_in_loop(block, lp)).collect();
        if blocks.iter().any(|&block| loops.innermost_loop(block) != Some(lp)) { return None; }
        let mut inside = CompoundBitSet::with_capacity(f.dfg.num_blocks());
        for block in &blocks { inside.insert(block.index()); }
        if header == f.layout.entry_block()? || cfg.pred_iter(header).all(|edge| inside.contains(edge.block.index())) { return None; }
        let (condition, first) = blocks.iter().filter_map(|&block| gate(f, cfg, used, f.layout.last_inst(block)?)).find(|(value, _)| {
            let definition = match f.dfg.value_def(*value) { ValueDef::Param(block, _) => Some(block),
                ValueDef::Result(inst, _) => f.layout.inst_block(inst), _ => None };
            definition.is_some_and(|block| !inside.contains(block.index()) && dom.block_dominates(block, header))
        })?;
        let gates: Vec<_> = blocks.iter().filter_map(|&block| gate(f, cfg, used, f.layout.last_inst(block)?)).collect();
        if gates.iter().any(|(value, gate)| *value != condition || gate.fast_truth != first.fast_truth
            || !inside.contains(gate.fast.index()) || !inside.contains(gate.slow.index())) { return None; }
        let gates = gates.into_iter().map(|(_, gate)| gate).collect();
        let mut exit = None;
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
                        if exit.is_some_and(|exit| exit != target) { return None; }
                        exit = Some(target);
                    }
                }
            }
        }
        let exit = exit?;
        // Explicit exit arguments already distinguish a zero-trip predecessor
        // from either loop copy. Implicit escaping values still require the
        // definition/exit dominance proofs below before adding join parameters.
        let mut escaping = CompoundBitSet::<usize>::with_capacity(f.dfg.num_values());
        for block in f.layout.blocks().filter(|block| !inside.contains(block.index())) {
            for inst in f.layout.block_insts(block) {
                for value in f.dfg.inst_values(inst).map(|value| f.dfg.resolve_aliases(value)) {
                    if defined.contains(value.index()) {
                        if !dom.block_dominates(exit, block) { return None; }
                        let definition = match f.dfg.value_def(value) { ValueDef::Param(block, _) => block,
                            ValueDef::Result(inst, _) => f.layout.inst_block(inst)?, _ => return None };
                        if !dom.block_dominates(definition, exit) { return None; }
                        escaping.insert(value.index());
                    }
                }
            }
        }
        Some(Self { header, blocks, inside, exit, condition, gates,
            escapes: escaping.iter().map(Value::new).collect() })
    }

    fn apply(self, f: &mut Function, cfg: &ControlFlowGraph) {
        f.dfg.resolve_all_aliases();
        let mut joins = SecondaryMap::<Value, Option<Value>>::with_capacity(f.dfg.num_values());
        for &value in &self.escapes { let ty = f.dfg.value_type(value); joins[value] = Some(f.dfg.append_block_param(self.exit, ty)); }
        let outside: Vec<_> = f.layout.blocks().filter(|block| !self.inside.contains(block.index())).collect();
        for block in outside { for inst in f.layout.block_insts(block).collect::<Vec<_>>() {
            f.dfg.map_inst_values(inst, |value| joins[value].unwrap_or(value));
        }}
        for &block in &self.blocks { for inst in f.layout.block_insts(block).collect::<Vec<_>>() {
            let dfg = &mut f.stencil.dfg;
            for branch in dfg.insts[inst].branch_destination_mut(&mut dfg.jump_tables, &mut dfg.exception_tables) {
                if branch.block(&dfg.value_lists) == self.exit {
                    for &value in &self.escapes { branch.append_argument(value, &mut dfg.value_lists); }
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
        cursor.ins().brif(self.condition, yes, &args, no, &args);
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
        let Some(plan) = loops.loops().find_map(|lp| Plan::find(f, &cfg, &dom, &loops, &used, lp)) else { break; };
        plan.apply(f, &cfg); changed = true;
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
    changed
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ir::{AbiParam, MemFlagsData, Signature, UserFuncName, types, condcodes::IntCC};

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
        let condition = if invariant { private } else { cursor.ins().icmp_imm(IntCC::Equal, count, 0) };
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
        let condition = cursor.ins().icmp_imm(IntCC::SignedGreaterThan, bound, 17);
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
        let query = cursor.ins().icmp_imm(IntCC::Equal, bound, 0);
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
