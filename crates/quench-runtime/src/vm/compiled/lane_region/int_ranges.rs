//! The integer ranges of a lane unit's registers, computed once before lowering: the int32
//! arithmetic whose exact result fits (no overflow exit) and the hoisted dense element
//! accesses whose index is non-negative and below their array's length (no bounds guard).
//!
//! One forward pass over the unit's PCs uses the transfer functions of `packed_ranges` (its
//! `State`, `result` and branch refinement), so the packed-field analysis and this one share
//! one meaning of a range. A loop header starts from the ranges its entering edge proves,
//! assumed unchanged (`Widening::Exact`), widened upward, or unknown, and every back edge
//! verifies the assumption; a failed one is relaxed and the pass repeats. A header never
//! assumes a range its back edges do not keep, so the pass is sound without a fixed point.

use super::packed_ranges::{result, State};
use super::*;
use crate::vm::operations::NumberArithmetic;

/// Passes before a unit proves nothing: each failed pass relaxes at least one header
/// assumption of a register, so a unit settles in a few and the bound guards compile time.
const WIDENING_ROUNDS: usize = 6;

/// What a loop header assumes of a register its entering edge proved a range of.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Widening {
    /// Upward-open: the entering range's minimum, up to int32's maximum.
    Upward,
    /// Only the register's type facts.
    Unknown,
}

/// The facts one analysis proves, by unit PC.
pub(super) struct IntRanges {
    overflow_free: CompoundBitSet,
    in_bounds: CompoundBitSet,
}

impl IntRanges {
    pub(super) fn new(instructions: usize) -> Self {
        Self { overflow_free: CompoundBitSet::with_capacity(instructions), in_bounds: CompoundBitSet::with_capacity(instructions) }
    }

    /// Whether the int32 addition, subtraction, product or step at `pc` has an exact int32
    /// result, whatever int32 operands reach it.
    pub(super) fn overflow_free(&self, pc: usize) -> bool { self.overflow_free.contains(pc) }

    /// Whether the index of the element access at `pc` lies in `0..length` of its array.
    pub(super) fn in_bounds(&self, pc: usize) -> bool { self.in_bounds.contains(pc) }
}

struct Analysis<'a> {
    program: &'a ResidualProgram,
    unit: &'a super::super::lane_unit::Unit,
    facts: &'a super::super::facts::Facts,
    entry: usize,
    headers: CompoundBitSet,
    generic: &'a [usize],
    /// Whether the element access at a PC reads or writes a hoisted dense view, whose miss
    /// leaves the body (`Lowering::dense_loop_access`).
    hoisted_dense: &'a dyn Fn(usize) -> bool,
}

type Assumptions = BTreeMap<usize, Vec<(Register, IntRange)>>;
type Relaxed = BTreeMap<(usize, Register), Widening>;

/// The ranges of `unit` entered at `entry`: `generic` are the PCs that run guest code natively.
pub(super) fn analyze(program: &ResidualProgram, unit: &super::super::lane_unit::Unit,
    facts: &super::super::facts::Facts, entry: usize, generic: &[usize],
    hoisted_dense: &dyn Fn(usize) -> bool) -> IntRanges {
    let mut headers = CompoundBitSet::with_capacity(unit.instructions.len());
    for (pc, instruction) in unit.instructions.iter().enumerate() {
        for target in successors(pc, *instruction).into_iter().flatten().filter(|target| *target <= pc) { headers.insert(target); }
    }
    let analysis = Analysis { program, unit, facts, entry, headers, generic, hoisted_dense };
    let mut relaxed = Relaxed::new();
    for _ in 0..WIDENING_ROUNDS {
        match analysis.pass(&relaxed) {
            Ok(ranges) => return ranges,
            Err(failures) => relaxed.extend(failures),
        }
    }
    IntRanges::new(unit.instructions.len())
}

impl Analysis<'_> {
    /// One forward pass under the header assumptions `relaxed`, or the assumptions a back
    /// edge did not keep, relaxed one step.
    fn pass(&self, relaxed: &Relaxed) -> Result<IntRanges, Vec<((usize, Register), Widening)>> {
        let len = self.unit.instructions.len();
        let mut proved = IntRanges::new(len);
        let mut incoming: Vec<Option<State>> = vec![None; len + 1];
        let (mut assumed, mut failures) = (Assumptions::new(), Vec::new());
        for pc in 0..len {
            let Some(mut state) = self.enter(pc, incoming[pc].take(), relaxed, &mut assumed) else { continue };
            self.visit(pc, &mut state, &mut proved);
            let instruction = self.unit.instructions[pc];
            let mut edges = successors(pc, instruction).into_iter().enumerate()
                .filter_map(|(edge, target)| target.map(|target| (edge, target))).peekable();
            let mut state = Some(state);
            while let Some((edge, target)) = edges.next() {
                let mut next = (if edges.peek().is_some() { state.clone() } else { state.take() }).expect("a state per edge");
                if next.branch(self.program, instruction, edge == 0).is_none() { continue; }
                if target > pc {
                    match &mut incoming[target] {
                        Some(known) => known.join(next),
                        slot => *slot = Some(next),
                    }
                } else {
                    self.verify(target, &next, (&assumed, relaxed), &mut failures);
                }
            }
        }
        if failures.is_empty() { Ok(proved) } else { Err(failures) }
    }

    /// The registers' ranges from their types alone: an int32 register holds some int32.
    fn fresh(&self, pc: usize) -> State {
        let types = self.facts.at(pc);
        State::new((0..usize::from(self.unit.registers)).map(|register|
            types.and_then(|types| types.get(register)).filter(|ty| ty.is_subtype(Type::INT32)).map(|_| IntRange::FULL)).collect())
    }

    /// The state at `pc` given the forward join `pre` of its entering edges: a header widens
    /// it by its assumptions, the entry (which an outside caller reaches) falls back to the
    /// types; any other PC takes `pre`, none where no forward edge reaches it.
    fn enter(&self, pc: usize, pre: Option<State>, relaxed: &Relaxed, assumed: &mut Assumptions) -> Option<State> {
        let header = self.headers.contains(pc);
        if !header && pc != self.entry { return pre; }
        let fresh = self.fresh(pc);
        let incoming = match pre {
            Some(mut pre) if pc == self.entry => { pre.join(fresh.clone()); Some(pre) }
            other => other,
        };
        let Some(mut state) = incoming else { return Some(fresh) };
        if !header { return Some(state); }
        let mut carried = Vec::new();
        for register in 0..self.unit.registers {
            let slot = usize::from(register);
            state.ranges[slot] = match (state.ranges[slot], relaxed.get(&(pc, register))) {
                (Some(known), None) => { carried.push((register, known)); Some(known) }
                (Some(known), Some(Widening::Upward)) => {
                    let widened = IntRange { min: known.min, max: i32::MAX };
                    carried.push((register, widened));
                    Some(widened)
                }
                _ => fresh.ranges[slot],
            };
        }
        state.relations = fresh.relations;
        assumed.insert(pc, carried);
        Some(state)
    }

    /// Records the assumptions of header `target` that the back edge's `next` state breaks.
    fn verify(&self, target: usize, next: &State, (assumed, relaxed): (&Assumptions, &Relaxed),
        failures: &mut Vec<((usize, Register), Widening)>) {
        for &(register, assumption) in assumed.get(&target).into_iter().flatten() {
            let kept = next.ranges[usize::from(register)];
            if kept.is_some_and(|range| assumption.contains(range)) { continue; }
            let key = (target, register);
            let level = if kept.is_some() && !relaxed.contains_key(&key) { Widening::Upward } else { Widening::Unknown };
            failures.push((key, level));
        }
    }

    /// Records what the proved state at `pc` proves of its instruction, then applies it.
    fn visit(&self, pc: usize, state: &mut State, proved: &mut IntRanges) {
        let unit = self.unit;
        if let Some(frame) = unit.markers.iter().position(|marker| *marker == pc).map(|index| unit.frames[index]) {
            for register in frame.base..frame.base + frame.registers { state.set(register, None, false); }
            for (to, from) in frame.passed_registers(self.program.function(frame.target.function as usize)) { state.copy(to, from); }
        }
        let instruction = unit.instructions[pc];
        if checked_arithmetic(instruction) && result(state, self.program, instruction).is_some() { proved.overflow_free.insert(pc); }
        if self.within_length(pc, instruction, state) { proved.in_bounds.insert(pc); }
        self.transfer(pc, instruction, state);
    }

    /// Whether the access at `pc` indexes below its array's length by a non-negative index.
    fn within_length(&self, pc: usize, instruction: WideInstruction, state: &State) -> bool {
        let (array, index) = match instruction.op().class() {
            Class::GetIndex => (instruction.operand_b().register_index(), instruction.operand_c().register_index()),
            Class::SetIndex => (Some(instruction.register_b()), Some(instruction.register_c())),
            _ => return false,
        };
        let Some((array, index)) = array.zip(index) else { return false };
        (self.hoisted_dense)(pc) && state.relations.below[usize::from(index)] == Some(array)
            && state.ranges[usize::from(index)].is_some_and(|range| range.min >= 0)
    }

    fn transfer(&self, pc: usize, instruction: WideInstruction, state: &mut State) {
        if instruction.op().is_move() && instruction.result_flags() == 0 {
            state.copy(instruction.result_register(), instruction.register_b());
        } else {
            let range = result(state, self.program, instruction);
            for register in writes(instruction) { state.set(register, None, false); }
            if let Some(range) = range { state.set(instruction.result_register(), Some(range), false); }
        }
        if self.disturbs_lengths(pc, instruction) { state.relations.clear(); }
        if super::super::reads_length(self.program, instruction) && instruction.result_flags() == 0
            && let Some(FieldLookup::Atom { base, .. }) = instruction.field_lookup()
            && let Some(array) = base.register_index().filter(|array| *array != instruction.result_register()) {
            state.relations.length_of[usize::from(instruction.result_register())] = Some(array);
        }
    }

    /// Whether `instruction` may change an array's length or run guest code that can: all but
    /// numeric steps, moves, jumps and reads of a hoisted dense view, whose misses leave.
    fn disturbs_lengths(&self, pc: usize, instruction: WideInstruction) -> bool {
        let class = instruction.op().class();
        let quiet = matches!(class, Class::Move | Class::LoadConst | Class::Nop | Class::Jump | Class::JumpTrue
                | Class::JumpFalse | Class::JumpBinaryFalse | Class::IncDec)
            || instruction.op().js_denotation() == crate::bytecode::JsDenotation::NumericBinary
            || (matches!(class, Class::GetIndex) && (self.hoisted_dense)(pc));
        !quiet || self.generic.contains(&pc)
    }
}

/// Whether `instruction` is int32 arithmetic whose overflow exit a range proof can remove.
fn checked_arithmetic(instruction: WideInstruction) -> bool {
    instruction.op().class().eq(Class::IncDec)
        || (instruction.op().js_denotation() == crate::bytecode::JsDenotation::NumericBinary
            && NumberArithmetic::from_js_instruction(instruction.op(), instruction.binary_operator())
                .is_some_and(|operation| matches!(operation, NumberArithmetic::Add | NumberArithmetic::Subtract | NumberArithmetic::Multiply)))
}
