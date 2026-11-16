//! The fixpoint of `transfer` over the control-flow graph, and the replay of its rows: the one
//! analysis, which every consumer of a type, a range or a length relation reads.

use super::lengths::Relations;
use super::ranges::{self, Edge};
use super::{set, transfer_observed, Contract, Facts, Flow, Observed, INLINE_ANALYSIS_ITEMS};
use crate::bytecode::{Constant, ControlFlowLayout, Register, ResidualProgram, WideInstruction};
use crate::typed_ir::{IntRange, Type};
use cranelift_codegen::bitset::CompoundBitSet;
use super::super::super::operations::NumberRelation;

type Pending = smallvec::SmallVec<[usize; INLINE_ANALYSIS_ITEMS]>;

/// The abstract state of a run at one point: the registers' types (a range is a type), and the
/// length relations a unit-proving run tracks beside them.
#[derive(Clone)]
pub(in crate::vm::compiled) struct Point {
    pub(super) types: smallvec::SmallVec<[Type; INLINE_ANALYSIS_ITEMS]>,
    pub(super) relations: Relations,
}

/// The unit a run proves the ranges of: its inlined frames, whose arguments the markers pass,
/// and the element accesses whose index it bounds.
#[derive(Clone, Copy)]
pub(in crate::vm::compiled) struct Model<'a> {
    pub(in crate::vm::compiled) program: &'a ResidualProgram,
    pub(in crate::vm::compiled) unit: &'a super::super::lane_unit::Unit,
    /// The PCs a previous body left at (`Lowering::generic`): their rows are unknown, and their
    /// instructions run guest code.
    pub(in crate::vm::compiled) generic: &'a [usize],
    /// Whether the element access at a PC reads or writes a hoisted view, whose miss leaves the
    /// body (`Lowering::hoisted_access`).
    pub(in crate::vm::compiled) dense: &'a dyn Fn(usize) -> bool,
}

/// What a run assumes beyond the program text.
#[derive(Clone, Copy)]
pub(in crate::vm::compiled) struct Seeds<'a> {
    /// The guarded block input contracts the fixed point keeps.
    pub(in crate::vm::compiled) contracts: &'a [Contract],
    /// The element reads observed or known to hold integers or Numbers.
    pub(in crate::vm::compiled) observed: Observed<'a>,
    /// The registers the entry guards to a range narrower than int32 (`Lowering::guard_entering_ranges`).
    pub(in crate::vm::compiled) entering: &'a [(Register, IntRange)],
    /// The unit the run proves ranges of; a type-only run has none and tracks no relations.
    pub(in crate::vm::compiled) model: Option<Model<'a>>,
    /// The PCs (ascending) of the products no consumer reads the sign of a zero of
    /// (`Lowering::signless_products`): where operands of ranges make a negative zero possible,
    /// the result is still an integer of their product's range.
    pub(in crate::vm::compiled) signless: &'a [usize],
}

impl<'a> Seeds<'a> {
    /// A type-only run that takes `observed` reads and no contract.
    pub(in crate::vm::compiled) fn observing(observed: Observed<'a>) -> Self {
        Self { contracts: &[], observed, entering: &[], model: None, signless: &[] }
    }
}

/// The program text a run reads.
pub(in crate::vm::compiled) struct Text<'a> {
    pub(in crate::vm::compiled) entry: usize,
    pub(in crate::vm::compiled) instructions: &'a [WideInstruction],
    pub(in crate::vm::compiled) leaders: &'a CompoundBitSet,
    pub(in crate::vm::compiled) constants: &'a [Constant],
    pub(in crate::vm::compiled) written: &'a dyn Fn(WideInstruction) -> Vec<Register>,
    pub(in crate::vm::compiled) own_shape: &'a dyn Fn(usize) -> Option<(Register, u32)>,
    pub(in crate::vm::compiled) native_calls: bool,
}

impl Point {
    /// The state at the entry: the checked inputs, an int32 one taking the range the entry
    /// guards where it has one.
    fn entering(initial: &[Type], seeds: Seeds<'_>) -> Self {
        let mut types: smallvec::SmallVec<[Type; INLINE_ANALYSIS_ITEMS]> = initial.iter().copied().collect();
        for &(register, range) in seeds.entering {
            if let Some(slot) = types.get_mut(usize::from(register))
                && let Some(narrowed) = slot.int_range().and_then(|known| known.intersect(range)) {
                *slot = Type::int(narrowed);
            }
        }
        let relations = seeds.model.map_or_else(Relations::default, |_| Relations::new(initial.len()));
        Self { types, relations }
    }

    /// The state on the edge of the comparison `instruction` that is taken where it is `truth`.
    /// An edge no value reaches keeps what it has: the lowering still branches to it.
    fn refine(&mut self, constants: &[Constant], instruction: WideInstruction, truth: bool) {
        let Some(relation) = NumberRelation::from_operator(instruction.binary_operator_field()) else { return };
        let operands = [instruction.operand_b(), instruction.operand_c()];
        self.relations.bound(ranges::effective_relation(relation, truth), operands);
        let Edge::Refined(refined) = Edge::of(&ranges::lookup(&self.types, constants), relation, truth, operands) else { return };
        for (operand, range) in operands.into_iter().zip(refined) {
            if let Some(register) = operand.register_index() { set(&mut self.types, register, Type::int(range)); }
        }
    }

    /// Whether the int32 arithmetic or step of `instruction` is exact in int32 here, whatever
    /// int32 operands reach it.
    pub(in crate::vm::compiled) fn overflow_free(&self, constants: &[Constant], instruction: WideInstruction) -> bool {
        ranges::exact(&ranges::lookup(&self.types, constants), instruction).is_some_and(|range| range.fits_int32())
    }

    /// The range of the result of the arithmetic of `instruction` where it leaves int32 but is
    /// an integer Number of `IntRange::WORD` (`ranges::index_range`).
    pub(in crate::vm::compiled) fn index_range(&self, constants: &[Constant], instruction: WideInstruction, signless: bool) -> Option<IntRange> {
        ranges::index_range(&ranges::lookup(&self.types, constants), instruction, signless)
    }

    /// Whether the product of `instruction` is no negative zero, whatever operands reach it.
    pub(in crate::vm::compiled) fn zero_free(&self, constants: &[Constant], instruction: WideInstruction) -> bool {
        ranges::zero_free(&ranges::lookup(&self.types, constants), instruction)
    }
}

impl Model<'_> {
    /// The inlined call whose marker is `pc` initializes the callee's registers: its window is
    /// unknown, and each passed argument is a copy of the caller's register.
    fn enter_frame(&self, pc: usize, point: &mut Point) {
        let Some(frame) = self.unit.markers.iter().position(|marker| *marker == pc).map(|index| self.unit.frames[index]) else { return };
        for register in frame.base..frame.base + frame.registers {
            set(&mut point.types, register, Type::ANY);
            point.relations.kill(register);
        }
        for (to, from) in frame.passed_registers(self.program.function(frame.target.function as usize)) {
            let copied = point.types.get(usize::from(from)).copied().unwrap_or(Type::ANY);
            set(&mut point.types, to, copied);
            point.relations.copy(to, from);
        }
    }
}

/// The state after the instruction at `pc` of a run, and where control goes.
fn step(point: &mut Point, text: &Text<'_>, seeds: Seeds<'_>, pc: usize) -> Flow {
    let instruction = text.instructions[pc];
    if let Some(model) = seeds.model {
        model.enter_frame(pc, point);
        point.relations.step(&model, pc, instruction, text.written);
    }
    // A direct own read proves its receiver before defining its result.
    // If result and receiver alias, transfer kills that refinement.
    if let Some((register, shape)) = (text.own_shape)(pc) { set(&mut point.types, register, Type::direct_object(shape)); }
    let signless = seeds.signless.binary_search(&pc).is_ok()
        .then(|| ranges::signless_range(&ranges::lookup(&point.types, text.constants), instruction)).flatten();
    let flow = transfer_observed(&mut point.types, instruction, text.constants, text.written, text.native_calls, seeds.observed.at(pc));
    if let Some(range) = signless { set(&mut point.types, instruction.result_register(), Type::int(range)); }
    flow
}

/// The block led by `leader` run from `point`, where `visit` sees the state before each
/// instruction; the PC of its last instruction and where control goes from it.
fn run_block(point: &mut Point, text: &Text<'_>, seeds: Seeds<'_>, leader: usize,
    mut visit: impl FnMut(usize, &Point)) -> (usize, Flow) {
    let mut pc = leader;
    loop {
        visit(pc, point);
        let flow = step(point, text, seeds, pc);
        let next = pc + 1;
        if !matches!(flow, Flow::Next) || next >= text.instructions.len() || text.leaders.contains(next) { return (pc, flow); }
        pc = next;
    }
}

impl Facts {
    /// The edge into the block led by `pc`, which joins the state `point` into its row.
    fn reach(&mut self, pc: usize, point: &Point, pending: &mut Pending, seeds: Seeds<'_>) {
        if !self.merge_point(pc, point) { return; }
        if seeds.model.is_some_and(|model| model.generic.contains(&pc)) { self.widen(pc); }
        pending.push(pc);
    }

    /// The edges out of a block that ended at `last` in `flow`, in the state `point`.
    fn follow(&mut self, text: &Text<'_>, seeds: Seeds<'_>, (last, flow): (usize, Flow), point: Point, pending: &mut Pending) {
        match flow {
            Flow::Leaves => {}
            Flow::Next => {
                if last + 1 < text.instructions.len() { self.reach(last + 1, &point, pending, seeds); }
            }
            Flow::To(target) => self.branch(text, seeds, (last, target), point, pending),
        }
    }

    /// The jump at `pc` to `target`, and the fallthrough of a conditional one, each edge in the
    /// state it proves.
    fn branch(&mut self, text: &Text<'_>, seeds: Seeds<'_>, (pc, target): (usize, usize), mut point: Point, pending: &mut Pending) {
        let instruction = text.instructions[pc];
        let mut following = (instruction.op().control_flow_layout() == ControlFlowLayout::ConditionalJump).then(|| point.clone());
        if instruction.op().is_jump_binary_false() {
            point.refine(text.constants, instruction, false);
            if let Some(following) = following.as_mut() { following.refine(text.constants, instruction, true); }
        }
        if target <= pc { self.headers.insert(target); }
        if target == text.entry && target <= pc { self.join_backedge(&point.types); }
        self.reach(target, &point, pending, seeds);
        if let Some(following) = following { self.reach(pc + 1, &following, pending, seeds); }
    }

    /// The join of the entry's backedge inputs, without the unknown external input.
    fn join_backedge(&mut self, types: &[Type]) {
        if self.backedge.is_empty() { self.backedge.extend_from_slice(types); return; }
        for (known, incoming) in self.backedge.iter_mut().zip(types) { *known = known.join(*incoming); }
    }

    /// Each reachable block replayed from its row, as the fixed point ran it: `visit` sees the
    /// state before each instruction.
    pub(in crate::vm::compiled) fn replay(&self, text: &Text<'_>, seeds: Seeds<'_>, mut visit: impl FnMut(usize, &Point)) {
        for start in (0..text.instructions.len()).filter(|pc| self.at(*pc).is_some()) {
            let mut point = self.point_at(start);
            run_block(&mut point, text, seeds, start, &mut visit);
        }
    }
}

/// The fixpoint of `transfer` over the CFG, seeded by checked region-entry facts.
pub(in crate::vm::compiled) fn analyze(entry: usize, initial: &[Type], instructions: &[WideInstruction],
    leaders: &CompoundBitSet, constants: &[Constant], written: &dyn Fn(WideInstruction) -> Vec<Register>,
    own_shape: &dyn Fn(usize) -> Option<(Register, u32)>, native_calls: bool, seeds: Seeds<'_>) -> Facts {
    analyze_text(&Text { entry, instructions, leaders, constants, written, own_shape, native_calls }, initial, seeds)
}

/// `analyze` over `text`.
pub(in crate::vm::compiled) fn analyze_text(text: &Text<'_>, initial: &[Type], seeds: Seeds<'_>) -> Facts {
    let mut facts = Facts::new(text.instructions.len(), initial.len(), seeds);
    let mut pending = Pending::new();
    if text.entry < text.instructions.len() {
        facts.reach(text.entry, &Point::entering(initial, seeds), &mut pending, seeds);
    }
    while let Some(leader) = pending.pop() {
        let mut point = facts.point_at(leader);
        let ended = run_block(&mut point, text, seeds, leader, |pc, _| { facts.reached.insert(pc); });
        facts.follow(text, seeds, ended, point, &mut pending);
    }
    facts
}
