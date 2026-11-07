//! Remove already-satisfied private spill stores across forward CFG joins.
//! Facts concern physical copies, not virtual-register names. Unvisited
//! predecessors (including backedges) contribute no facts; no fixed point is
//! required. Allocation, edit order, and recovery points are unchanged.

use alloc::{boxed::Box, vec, vec::Vec};
use crate::bitset::CompoundBitSet;
use regalloc2::{Allocation, Block, Edit, Function, Inst, InstOrEdit, MachineEnv,
    OperandKind, Output, PReg, PRegSet, RegClass};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Spill {
    start: usize,
    end: usize,
    class: RegClass,
}

impl Spill {
    fn new(function: &impl Function, allocation: Allocation, class: RegClass) -> Self {
        let index = allocation.as_stack().unwrap().index();
        let size = function.spillslot_size(class);
        let start = if function.multi_spillslot_named_by_last_slot() { index + 1 - size } else { index };
        Self { start, end: start + size, class }
    }

    fn overlaps(self, other: Self) -> bool { self.start < other.end && other.start < self.end }
}

type Facts = [Option<Spill>; PReg::NUM_INDEX];

fn write_spill(facts: &mut Facts, spill: Spill) {
    for fact in facts.iter_mut() {
        if fact.is_some_and(|known| known.overlaps(spill)) { *fact = None; }
    }
}

/// Transfer the allocator's resolved, sequential moves. Only stores whose
/// source is already equal to their entire destination footprint are removed.
fn transfer(function: &impl Function, allocatable: &PRegSet,
    facts: &mut Facts, from: Allocation, to: Allocation) -> bool {
    match (from.as_reg(), to.as_reg()) {
        (Some(source), None) if to.is_stack() => {
            let spill = Spill::new(function, to, source.class());
            if allocatable.contains(source) && facts[source.index()] == Some(spill) { return true; }
            write_spill(facts, spill);
            if allocatable.contains(source) { facts[source.index()] = Some(spill); }
        }
        (None, Some(destination)) if from.is_stack() => {
            facts[destination.index()] = allocatable.contains(destination)
                .then(|| Spill::new(function, from, destination.class()));
        }
        (Some(source), Some(destination)) => {
            facts[destination.index()] = if source.class() == destination.class()
                && allocatable.contains(source) && allocatable.contains(destination) {
                facts[source.index()]
            } else { None };
        }
        _ => facts.fill(None),
    }
    false
}

pub(super) fn eliminate(function: &impl Function, environment: &MachineEnv,
    output: &mut Output, is_safepoint: impl Fn(Inst) -> bool) {
    if !output.edits.iter().any(|(_, Edit::Move { from, to })| from.is_reg() && to.is_stack()) { return; }
    let mut allocatable = PRegSet::default();
    for reg in environment.preferred_regs_by_class.iter()
        .chain(environment.non_preferred_regs_by_class.iter()).flatten() {
        allocatable.add(reg);
    }
    // A block's exit facts live from its end until its last forward successor
    // merges them, held as the few registers they name.
    let mut exits: Vec<Option<Box<[(u8, Spill)]>>> = vec![None; function.num_blocks()];
    let mut remove = CompoundBitSet::<usize>::with_capacity(output.edits.len());
    for index in 0..function.num_blocks() {
        let block = Block::new(index);
        let predecessors = function.block_preds(block);
        let mut facts = [None; PReg::NUM_INDEX];
        // Index order is the traversal authority. None for an earlier block
        // means a visited empty exit, not an unresolved predecessor.
        if block != function.entry_block() && !predecessors.is_empty()
            && predecessors.iter().all(|pred| pred.index() < index)
            && let Some(initial) = &exits[predecessors[0].index()] {
            for &(reg, spill) in initial.iter() { facts[usize::from(reg)] = Some(spill); }
            for pred in &predecessors[1..] {
                let Some(other) = &exits[pred.index()] else { facts.fill(None); break; };
                let mut known = other.iter().peekable();
                for (reg, fact) in facts.iter_mut().enumerate() {
                    let agreed = known.next_if(|(other, _)| usize::from(*other) == reg).map(|(_, spill)| *spill);
                    if *fact != agreed { *fact = None; }
                }
            }
        }
        for pred in predecessors {
            if pred.index() < index && function.block_succs(*pred).iter().all(|succ| succ.index() <= index) {
                exits[pred.index()] = None;
            }
        }
        let first = regalloc2::ProgPoint::before(function.block_insns(block).first());
        let mut edit_index = output.edits.partition_point(|(point, _)| *point < first);
        for item in output.block_insts_and_edits(function, block) {
            match item {
                InstOrEdit::Edit(Edit::Move { from, to }) => {
                    if transfer(function, &allocatable, &mut facts, *from, *to) { remove.insert(edit_index); }
                    edit_index += 1;
                }
                InstOrEdit::Inst(inst) => {
                    if is_safepoint(inst) { facts.fill(None); }
                    for (operand, allocation) in function.inst_operands(inst).iter().zip(output.inst_allocs(inst)) {
                        if operand.kind() == OperandKind::Def {
                            if let Some(reg) = allocation.as_reg() { facts[reg.index()] = None; }
                            else if allocation.is_stack() {
                                write_spill(&mut facts, Spill::new(function, *allocation, operand.class()));
                            }
                        }
                    }
                    for reg in function.inst_clobbers(inst) { facts[reg.index()] = None; }
                }
            }
        }
        if facts.iter().any(Option::is_some) {
            exits[index] = Some(facts.iter().enumerate()
                .filter_map(|(reg, fact)| Some((u8::try_from(reg).ok()?, (*fact)?))).collect());
        }
    }
    let mut index = 0;
    output.edits.retain(|_| { let keep = !remove.contains(index); index += 1; keep });
    output.stats.edits_count = output.edits.len();
}

#[cfg(test)]
mod tests {
    use super::*;
    use regalloc2::{InstRange, Operand, ProgPoint, SpillSlot, VReg};

    struct Diamond {
        preds: [Vec<Block>; 4],
        succs: [Vec<Block>; 4],
        defs: Vec<Operand>,
        clobber: PRegSet,
        last_slot: bool,
    }
    impl Diamond {
        fn new() -> Self {
            Self { preds: [vec![], vec![Block::new(0)], vec![Block::new(0)],
                vec![Block::new(1), Block::new(2)]],
                succs: [vec![Block::new(1), Block::new(2)], vec![Block::new(3)], vec![Block::new(3)], vec![]],
                defs: vec![], clobber: PRegSet::default(), last_slot: false }
        }
    }
    impl Function for Diamond {
        fn num_insts(&self) -> usize { 8 }
        fn num_blocks(&self) -> usize { 4 }
        fn entry_block(&self) -> Block { Block::new(0) }
        fn block_insns(&self, block: Block) -> InstRange {
            InstRange::new(Inst::new(block.index() * 2), Inst::new(block.index() * 2 + 2))
        }
        fn block_succs(&self, block: Block) -> &[Block] { &self.succs[block.index()] }
        fn block_preds(&self, block: Block) -> &[Block] { &self.preds[block.index()] }
        fn block_params(&self, _: Block) -> &[VReg] { &[] }
        fn is_ret(&self, inst: Inst) -> bool { inst.index() == 7 }
        fn is_branch(&self, inst: Inst) -> bool { inst.index() % 2 == 1 && inst.index() != 7 }
        fn branch_blockparams(&self, _: Block, _: Inst, _: usize) -> &[VReg] { &[] }
        fn inst_operands(&self, inst: Inst) -> &[Operand] { if inst.index() == 5 { &self.defs } else { &[] } }
        fn inst_clobbers(&self, inst: Inst) -> PRegSet { if inst.index() == 5 { self.clobber } else { PRegSet::default() } }
        fn num_vregs(&self) -> usize { 1 }
        fn spillslot_size(&self, class: RegClass) -> usize { if class == RegClass::Int { 1 } else { 2 } }
        fn multi_spillslot_named_by_last_slot(&self) -> bool { self.last_slot }
    }
    fn reg(index: usize) -> Allocation { Allocation::reg(PReg::new(index, RegClass::Int)) }
    fn slot(index: usize) -> Allocation { Allocation::stack(SpillSlot::new(index)) }
    fn movement(inst: usize, from: Allocation, to: Allocation) -> (ProgPoint, Edit) {
        (ProgPoint::before(Inst::new(inst)), Edit::Move { from, to })
    }
    fn environment() -> MachineEnv {
        let mut integers = PRegSet::default();
        integers.add(PReg::new(0, RegClass::Int)); integers.add(PReg::new(1, RegClass::Int));
        MachineEnv { preferred_regs_by_class: [integers, PRegSet::default(), PRegSet::default()],
            non_preferred_regs_by_class: [PRegSet::default(); 3], scratch_by_class: [None; 3], fixed_stack_slots: vec![] }
    }
    fn output() -> Output {
        Output { edits: vec![movement(2, slot(0), reg(0)), movement(4, slot(0), reg(0)), movement(6, reg(0), slot(0))],
            inst_alloc_offsets: vec![0; 8], ..Output::default() }
    }

    #[test]
    fn diamond_eliminates_only_a_store_proven_on_every_incoming_path() {
        let function = Diamond::new();
        let mut code = output();
        eliminate(&function, &environment(), &mut code, |_| false);
        assert_eq!(code.edits.len(), 2);
        for replacement in [movement(4, slot(1), reg(0)), movement(4, slot(0), reg(1))] {
            let mut code = output(); code.edits[1] = replacement;
            eliminate(&function, &environment(), &mut code, |_| false);
            assert_eq!(code.edits.len(), 3);
        }
    }

    #[test]
    fn writes_clobbers_safepoints_and_unfinished_edges_retire_facts() {
        for case in 0..5 {
            let mut function = Diamond::new(); let mut code = output();
            match case {
                0 => function.clobber.add(PReg::new(0, RegClass::Int)),
                1 => { function.defs.push(Operand::reg_def(VReg::new(0, RegClass::Int)));
                    code.allocs.push(reg(0)); code.inst_alloc_offsets[6..].fill(1); }
                2 => function.preds[3].push(Block::new(3)),
                3 => code.edits.insert(2, movement(5, reg(1), slot(0))),
                _ => {},
            }
            let before = code.edits.len();
            eliminate(&function, &environment(), &mut code, |inst| case == 4 && inst.index() == 5);
            assert_eq!(code.edits.len(), before, "case {case}");
        }
    }

    #[test]
    fn sequential_moves_respect_scratch_and_overlapping_spill_widths() {
        let function = Diamond::new(); let mut facts = [None; PReg::NUM_INDEX];
        let mut allocatable = PRegSet::default(); allocatable.add(PReg::new(0, RegClass::Int));
        transfer(&function, &allocatable, &mut facts, slot(1), reg(0));
        let vector = Allocation::reg(PReg::new(0, RegClass::Vector));
        transfer(&function, &allocatable, &mut facts, vector, slot(0));
        assert!(!transfer(&function, &allocatable, &mut facts, reg(0), slot(1)));
        // A reserved scratch cannot carry facts across an original instruction.
        transfer(&function, &allocatable, &mut facts, slot(1), reg(1));
        transfer(&function, &allocatable, &mut facts, reg(1), reg(0));
        assert!(!transfer(&function, &allocatable, &mut facts, reg(0), slot(1)));
    }

    #[test]
    fn resolved_swap_order_and_last_named_spill_extents_are_preserved() {
        let mut function = Diamond::new();
        let mut facts = [None; PReg::NUM_INDEX];
        let mut allocatable = PRegSet::default();
        allocatable.add(PReg::new(0, RegClass::Int)); allocatable.add(PReg::new(1, RegClass::Int));
        transfer(&function, &allocatable, &mut facts, slot(0), reg(0));
        transfer(&function, &allocatable, &mut facts, slot(1), reg(1));
        // Resolved swap uses reserved register 2 as scratch.
        transfer(&function, &allocatable, &mut facts, reg(0), reg(2));
        transfer(&function, &allocatable, &mut facts, reg(1), reg(0));
        transfer(&function, &allocatable, &mut facts, reg(2), reg(1));
        assert!(transfer(&function, &allocatable, &mut facts, reg(0), slot(1)));
        assert!(!transfer(&function, &allocatable, &mut facts, reg(1), slot(1)));
        function.last_slot = true;
        transfer(&function, &allocatable, &mut facts, slot(0), reg(0));
        let vector = Allocation::reg(PReg::new(0, RegClass::Vector));
        transfer(&function, &allocatable, &mut facts, vector, slot(1));
        assert!(!transfer(&function, &allocatable, &mut facts, reg(0), slot(0)));
    }
}
