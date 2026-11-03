//! Unit-stride integer induction, derived from verified SSA edge conditions.
//! The source counter remains `bound - remaining` modulo its word width, so
//! exits keep exact values while the repeating path carries only the distance.

use crate::cursor::{Cursor, FuncCursor};
use crate::bitset::CompoundBitSet;
use crate::dominator_tree::DominatorTree;
use crate::flowgraph::ControlFlowGraph;
use crate::ir::{self, Block, BlockArg, Function, Inst, InstBuilder, InstructionData, Opcode, Value, ValueDef};
use crate::ir::condcodes::IntCC;
use crate::loop_analysis::{Loop, LoopAnalysis};
use alloc::vec::Vec;
use core::ops::Range;
use cranelift_entity::{EntityRef, packed_option::ReservedValue};
use smallvec::SmallVec;

/// Common preheader/latch storage; larger edge sets spill without a limit.
const INLINE_EDGES: usize = 2;

struct Uses {
    ranges: Vec<Range<usize>>,
    instructions: Vec<Inst>,
}

impl Uses {
    fn new(f: &Function) -> Self {
        let mut counts = vec![0; f.dfg.num_values()];
        for block in f.layout.blocks() { for inst in f.layout.block_insts(block) {
            for value in f.dfg.inst_values(inst) { counts[f.dfg.resolve_aliases(value).index()] += 1; }
        }}
        let mut total = 0;
        let ranges = counts.iter_mut().map(|count| {
            let start = total;
            total += *count;
            *count = start;
            start..total
        }).collect();
        let mut instructions = vec![Inst::reserved_value(); total];
        for block in f.layout.blocks() { for inst in f.layout.block_insts(block) {
            for value in f.dfg.inst_values(inst) {
                let index = f.dfg.resolve_aliases(value).index();
                instructions[counts[index]] = inst;
                counts[index] += 1;
            }
        }}
        Self { ranges, instructions }
    }

    fn at(&self, value: Value) -> &[Inst] { &self.instructions[self.ranges[value.index()].clone()] }
}

fn integer(f: &Function, value: Value) -> Option<(ir::Type, u64)> {
    let value = f.dfg.resolve_aliases(value);
    let ValueDef::Result(inst, _) = f.dfg.value_def(value) else { return None; };
    let InstructionData::UnaryImm { opcode: Opcode::Iconst, imm } = f.dfg.insts[inst] else { return None; };
    let ty = f.dfg.value_type(value);
    if !ty.is_int() || ty.bits() > u64::BITS { return None; }
    Some((ty, imm.bits() as u64 & (u64::MAX >> (u64::BITS - ty.bits()))))
}

fn same(f: &Function, a: Value, b: Value) -> bool {
    f.dfg.resolve_aliases(a) == f.dfg.resolve_aliases(b)
        || integer(f, a).is_some_and(|constant| Some(constant) == integer(f, b))
}

/// Which truth value of `condition` establishes the strict signed relation?
fn less_condition(f: &Function, mut condition: Value, left: Value, right: Value) -> Option<bool> {
    let mut polarity = true;
    loop {
        let ValueDef::Result(inst, _) = f.dfg.value_def(f.dfg.resolve_aliases(condition)) else { return None; };
        let InstructionData::IntCompare { cond, args, .. } = f.dfg.insts[inst] else { return None; };
        if matches!(cond, IntCC::Equal | IntCC::NotEqual) {
            if integer(f, left).is_some_and(|(ty, bits)| bits == 1_u64 << (ty.bits() - 1))
                && ((same(f, args[0], left) && same(f, args[1], right))
                    || (same(f, args[1], left) && same(f, args[0], right))) {
                return Some(if cond == IntCC::NotEqual { polarity } else { !polarity });
            }
            let next = if integer(f, args[1]).is_some_and(|(_, bits)| bits == 0) { args[0] }
                else if integer(f, args[0]).is_some_and(|(_, bits)| bits == 0) { args[1] }
                else { return None; };
            polarity ^= cond == IntCC::Equal;
            condition = next;
            continue;
        }
        return match cond {
            IntCC::SignedLessThan if same(f, args[0], left) && same(f, args[1], right) => Some(polarity),
            IntCC::SignedGreaterThanOrEqual if same(f, args[0], left) && same(f, args[1], right) => Some(!polarity),
            IntCC::SignedGreaterThan if same(f, args[1], left) && same(f, args[0], right) => Some(polarity),
            IntCC::SignedLessThanOrEqual if same(f, args[1], left) && same(f, args[0], right) => Some(!polarity),
            _ => None,
        };
    }
}

fn proves_less(f: &Function, cfg: &ControlFlowGraph, dom: &DominatorTree,
    predecessor: Block, destination: Block, left: Value, right: Value) -> bool {
    if let (Some((ty, a)), Some((_, b))) = (integer(f, left), integer(f, right)) {
        let shift = u64::BITS - ty.bits();
        if ((a << shift) as i64 >> shift) < ((b << shift) as i64 >> shift) { return true; }
    }
    let mut current = Some(predecessor);
    while let Some(block) = current {
        if let Some(inst) = f.layout.last_inst(block)
            && let InstructionData::Brif { arg, blocks, .. } = f.dfg.insts[inst]
            && let Some(truth) = less_condition(f, arg, left, right) {
            let pass = blocks[usize::from(!truth)].block(&f.dfg.value_lists);
            let fail = blocks[usize::from(truth)].block(&f.dfg.value_lists);
            if pass != fail {
                if block == predecessor && pass == destination { return true; }
                let mut incoming = cfg.pred_iter(pass);
                if Some(pass) != f.layout.entry_block()
                    && incoming.next().is_some_and(|edge| edge.block == block && edge.inst == inst)
                    && incoming.next().is_none() && dom.block_dominates(pass, predecessor) {
                    return true;
                }
            }
        }
        current = dom.idom(block);
    }
    false
}

#[derive(Clone, Copy)]
struct Edge { inst: Inst, index: usize, back: bool }

struct Plan {
    header: Block,
    parameter: Value,
    slot: usize,
    start: Value,
    bound: Value,
    update: Inst,
    comparisons: SmallVec<[(Inst, bool); INLINE_EDGES]>,
    edges: SmallVec<[Edge; INLINE_EDGES]>,
}

impl Plan {
    fn find(f: &Function, cfg: &ControlFlowGraph, dom: &DominatorTree,
        loops: &LoopAnalysis, lp: Loop, parameter: Value, uses: &Uses) -> Option<Self> {
        let header = loops.loop_header(lp);
        let ValueDef::Param(_, slot) = f.dfg.value_def(parameter) else { return None; };
        let ty = f.dfg.value_type(parameter);
        if !ty.is_int() || ty.bits() > u64::BITS { return None; }
        let mut start = None;
        let mut next = None;
        let mut edges = SmallVec::<[Edge; INLINE_EDGES]>::new();
        for pred in cfg.pred_iter(header) {
            for (index, branch) in f.dfg.insts[pred.inst].branch_destination(&f.dfg.jump_tables, &f.dfg.exception_tables).iter().enumerate() {
                if branch.block(&f.dfg.value_lists) != header { continue; }
                let value = f.dfg.resolve_aliases(branch.args(&f.dfg.value_lists).nth(slot)?.as_value()?);
                let back = loops.is_in_loop(pred.block, lp);
                if back {
                    if next.is_some_and(|old| old != value) { return None; }
                    next = Some(value);
                } else {
                    if start.is_some_and(|old| !same(f, old, value)) { return None; }
                    start = Some(value);
                }
                edges.push(Edge { inst: pred.inst, index, back });
            }
        }
        let (start, next) = (start?, next?);
        let ValueDef::Result(update, _) = f.dfg.value_def(next) else { return None; };
        let InstructionData::Binary { opcode: Opcode::Iadd, args } = f.dfg.insts[update] else { return None; };
        if !((same(f, args[0], parameter) && integer(f, args[1]) == Some((ty, 1)))
            || (same(f, args[1], parameter) && integer(f, args[0]) == Some((ty, 1)))) { return None; }
        let bound = uses.at(next).iter().find_map(|inst| {
            let block = f.layout.inst_block(*inst)?;
            if !loops.is_in_loop(block, lp) { return None; }
            match f.dfg.insts[*inst] {
                InstructionData::IntCompare { cond: IntCC::SignedLessThan | IntCC::SignedGreaterThanOrEqual, args, .. }
                    if same(f, args[0], next) => Some(f.dfg.resolve_aliases(args[1])),
                InstructionData::IntCompare { cond: IntCC::SignedGreaterThan | IntCC::SignedLessThanOrEqual, args, .. }
                    if same(f, args[1], next) => Some(f.dfg.resolve_aliases(args[0])),
                _ => None,
            }
        })?;
        if f.dfg.value_type(bound) != ty { return None; }
        for invariant in [start, bound] {
            if integer(f, invariant).is_some() { continue; }
            let block = match f.dfg.value_def(invariant) {
                ValueDef::Param(block, _) => block,
                ValueDef::Result(inst, _) => f.layout.inst_block(inst)?,
                _ => return None,
            };
            if loops.is_in_loop(block, lp) || !dom.block_dominates(block, header) { return None; }
        }
        for edge in &edges {
            let pred = f.layout.inst_block(edge.inst)?;
            let left = if edge.back { next } else { start };
            if !proves_less(f, cfg, dom, pred, header, left, bound) { return None; }
        }
        let mut comparisons = SmallVec::new();
        for (value, advancing) in [(parameter, false), (next, true)] {
            for &inst in uses.at(value) {
                let block = f.layout.inst_block(inst)?;
                if !loops.is_in_loop(block, lp) || inst == update { continue; }
                if advancing && let Some(&test) = f.dfg.inst_results(inst).first()
                    && let Some(truth) = less_condition(f, test, next, bound) {
                    if !comparisons.iter().any(|(old, _)| *old == inst) { comparisons.push((inst, truth)); }
                    continue;
                }
                // Branch-only uses are the carried value in its own phi slot.
                // Other hot uses would require computing the source counter.
                if f.dfg.inst_args(inst).iter().any(|arg| same(f, *arg, value)) { return None; }
                let mut carried = false;
                for branch in f.dfg.insts[inst].branch_destination(&f.dfg.jump_tables, &f.dfg.exception_tables) {
                    for (index, arg) in branch.args(&f.dfg.value_lists).enumerate() {
                        if arg.as_value().is_some_and(|arg| same(f, arg, value)) {
                            if !advancing || index != slot || branch.block(&f.dfg.value_lists) != header { return None; }
                            carried = true;
                        }
                    }
                }
                if !carried { return None; }
            }
        }
        Some(Self { header, parameter, slot, start, bound, update, comparisons, edges })
    }

    fn apply(self, f: &mut Function, uses: &Uses, loops: &LoopAnalysis) {
        let ty = f.dfg.value_type(self.parameter);
        let lp = loops.is_loop_header(self.header).expect("verified counted loop");
        let source_next = f.dfg.first_result(self.update);
        let one = f.dfg.inst_args(self.update).iter().copied()
            .find(|value| integer(f, *value) == Some((ty, 1))).expect("unit stride");
        // Constants can be materialized before every entry without a guard.
        for value in [self.start, self.bound] {
            if integer(f, value).is_none() { continue; }
            let ValueDef::Result(inst, _) = f.dfg.value_def(value) else { unreachable!() };
            f.layout.remove_inst(inst);
            let first = f.layout.first_inst(f.layout.entry_block().unwrap()).unwrap();
            f.layout.insert_inst(inst, first);
        }
        let remaining = f.dfg.replace_block_param(self.parameter, ty);
        // Reuse the update's machine value, after redirecting its semantic
        // source-counter uses below. It now denotes the remaining distance.
        f.dfg.insts[self.update] = InstructionData::Binary { opcode: Opcode::Isub, args: [remaining, one] };
        let mut cursor = FuncCursor::new(f);
        cursor.goto_inst(self.update);
        let zero = cursor.ins().iconst(ty, 0);
        for (inst, truth) in self.comparisons {
            cursor.func.dfg.insts[inst] = InstructionData::IntCompare {
                opcode: Opcode::Icmp, cond: if truth { IntCC::NotEqual } else { IntCC::Equal }, args: [source_next, zero] };
        }
        for edge in self.edges {
            let argument = if edge.back { source_next } else if integer(cursor.func, self.start).is_some_and(|(_, bits)| bits == 0) {
                self.bound
            } else {
                cursor.goto_inst(edge.inst);
                cursor.ins().isub(self.bound, self.start)
            };
            let dfg = &mut cursor.func.dfg;
            let branch = &mut dfg.insts[edge.inst].branch_destination_mut(&mut dfg.jump_tables, &mut dfg.exception_tables)[edge.index];
            let mut index = 0;
            branch.update_args(&mut dfg.value_lists, |old| {
                let value = if index == self.slot { BlockArg::Value(argument) } else { old };
                index += 1;
                value
            });
        }
        // Emit semantic projections only in exit blocks that actually use
        // them. Labels follow those exact values, never the countdown bits.
        for (source, distance) in [(self.parameter, remaining), (source_next, source_next)] {
            // Egraph clones can refer to the original value's label names.
            // Detach those names before the original ID changes meaning.
            if let Some(labels) = cursor.func.dfg.values_labels.as_mut()
                && let Some(assignment) = labels.get(&source).cloned() {
                for existing in labels.values_mut() {
                    if matches!(existing, ir::ValueLabelAssignments::Alias { value, .. } if *value == source) {
                        *existing = assignment.clone();
                    }
                }
            }
            let assignment = cursor.func.dfg.values_labels.as_mut().and_then(|labels| labels.remove(&source));
            let mut projections = vec![None; cursor.func.dfg.num_blocks()];
            let mut reused_parameter = false;
            for &inst in uses.at(source) {
                let block = cursor.func.layout.inst_block(inst).expect("live source use");
                if loops.is_in_loop(block, lp) { continue; }
                let projected = match projections[block.index()] {
                    Some(value) => value,
                    None => {
                        cursor.goto_first_inst(block);
                        let value = if source == self.parameter && !reused_parameter {
                            reused_parameter = true;
                            cursor.ins().with_result(self.parameter).isub(self.bound, distance)
                        } else { cursor.ins().isub(self.bound, distance) };
                        if let Some(assignment) = &assignment {
                            cursor.func.dfg.values_labels.as_mut().expect("source labels").insert(value, assignment.clone());
                        }
                        projections[block.index()] = Some(value);
                        value
                    }
                };
                cursor.func.dfg.map_inst_values(inst, |value| if value == source { projected } else { value });
            }
        }
    }
}

pub(crate) fn run(f: &mut Function, cfg: &ControlFlowGraph, dom: &DominatorTree, loops: &LoopAnalysis) {
    if loops.loops().next().is_none() { return; }
    let uses = Uses::new(f);
    let mut plans = Vec::new();
    for lp in loops.loops() {
        for &parameter in f.dfg.block_params(loops.loop_header(lp)) {
            if let Some(plan) = Plan::find(f, cfg, dom, loops, lp, parameter, &uses) { plans.push(plan); }
        }
    }
    if !plans.is_empty() {
        // Plans share one immutable SSA snapshot. Select independent views;
        // a producer cannot repurpose a value another selected plan captured.
        let mut outputs = CompoundBitSet::<usize>::with_capacity(f.dfg.num_values());
        let mut inputs = CompoundBitSet::<usize>::with_capacity(f.dfg.num_values());
        for plan in plans {
            let next = f.dfg.first_result(plan.update);
            if outputs.contains(plan.bound.index()) || outputs.contains(plan.start.index())
                || inputs.contains(plan.parameter.index()) || inputs.contains(next.index()) { continue; }
            outputs.insert(plan.parameter.index()); outputs.insert(next.index());
            inputs.insert(plan.bound.index()); inputs.insert(plan.start.index());
            plan.apply(f, &uses, loops);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ir::{AbiParam, MemFlagsData, Signature, UserFuncName, types};

    #[derive(Clone, Copy)]
    enum Shape { Guarded, Unchecked, Reconverged, EntrySelf, Bypassed, StepTwo, NonStrict, HotUse, StaticBound }

    fn candidate(shape: Shape, dynamic_start: bool) -> bool {
        let mut signature = Signature::new(crate::isa::CallConv::SystemV);
        signature.params.extend([AbiParam::new(types::I32), AbiParam::new(types::I64), AbiParam::new(types::I32)]);
        signature.returns.push(AbiParam::new(types::I32));
        let mut f = Function::with_name_signature(UserFuncName::testcase("countdown_edges"), signature);
        let entry = f.dfg.make_block();
        let fail = f.dfg.make_block();
        let preheader = f.dfg.make_block();
        let header = f.dfg.make_block();
        let compare = f.dfg.make_block();
        let latch = f.dfg.make_block();
        let exit = f.dfg.make_block();
        let bound = f.dfg.append_block_param(entry, types::I32);
        let pointer = f.dfg.append_block_param(entry, types::I64);
        let start = f.dfg.append_block_param(entry, types::I32);
        let counter = f.dfg.append_block_param(header, types::I32);
        let mut cursor = FuncCursor::new(&mut f);
        cursor.insert_block(entry);
        let start = if dynamic_start { start } else { cursor.ins().iconst(types::I32, 0) };
        let bound = if matches!(shape, Shape::StaticBound) { cursor.ins().iconst(types::I32, 17) } else { bound };
        if matches!(shape, Shape::Unchecked | Shape::StaticBound) {
            cursor.ins().jump(preheader, &[]);
        } else {
            let condition = cursor.ins().icmp(IntCC::SignedLessThan, start, bound);
            let inverted = cursor.ins().icmp_imm(IntCC::Equal, condition, 0);
            if matches!(shape, Shape::EntrySelf) { cursor.ins().brif(condition, entry, &[], preheader, &[]); }
            else { cursor.ins().brif(inverted, fail, &[], preheader, &[]); }
        }
        cursor.insert_block(fail);
        if matches!(shape, Shape::Reconverged) { cursor.ins().jump(preheader, &[]); }
        else { cursor.ins().return_(&[start]); }
        cursor.insert_block(preheader);
        cursor.ins().jump(header, &[start.into()]);
        cursor.insert_block(header);
        if matches!(shape, Shape::HotUse) { cursor.ins().store(MemFlagsData::trusted(), counter, pointer, 0); }
        let next = cursor.ins().iadd_imm(counter, if matches!(shape, Shape::StepTwo) { 2 } else { 1 });
        cursor.ins().jump(compare, &[]);
        cursor.insert_block(compare);
        let condition = cursor.ins().icmp(if matches!(shape, Shape::NonStrict) { IntCC::SignedLessThanOrEqual }
            else { IntCC::SignedLessThan }, next, bound);
        cursor.ins().brif(condition, latch, &[], exit, &[]);
        cursor.insert_block(latch);
        cursor.ins().jump(header, &[next.into()]);
        cursor.insert_block(exit);
        if matches!(shape, Shape::Bypassed) { cursor.ins().jump(latch, &[]); }
        else { cursor.ins().return_(&[next]); }
        let cfg = ControlFlowGraph::with_function(&f);
        let dom = DominatorTree::with_function(&f, &cfg);
        let mut loops = LoopAnalysis::new();
        loops.compute(&f, &cfg, &dom);
        let lp = loops.is_loop_header(header).expect("fixture loop");
        Plan::find(&f, &cfg, &dom, &loops, lp, counter, &Uses::new(&f)).is_some()
    }

    #[test]
    fn entry_and_every_backedge_need_strict_unit_stride_proofs() {
        for dynamic in [false, true] {
            assert!(candidate(Shape::Guarded, dynamic));
            assert_eq!(candidate(Shape::StaticBound, dynamic), !dynamic);
            for shape in [Shape::Unchecked, Shape::Reconverged, Shape::EntrySelf, Shape::Bypassed,
                Shape::StepTwo, Shape::NonStrict, Shape::HotUse] { assert!(!candidate(shape, dynamic)); }
        }
    }
}
