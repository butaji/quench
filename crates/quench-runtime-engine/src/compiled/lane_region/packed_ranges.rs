//! Conditional numeric refinement of one admitted packed field. The loop's
//! existing alias plan owns object identity; this analysis owns numeric bounds.
use super::*;
use super::super::facts::ranges::{self, Edge};
use quench_runtime::vm::operations::NumberRelation;

#[derive(Clone)]
pub(super) struct State {
    field: IntRange,
    ranges: Vec<Option<IntRange>>,
    /// Registers equal to the current field, invalidated by every field write.
    current: CompoundBitSet,
}

impl State {
    pub(super) fn new(ranges: Vec<Option<IntRange>>) -> Self {
        let registers = ranges.len();
        Self { field: IntRange::packed(), current: CompoundBitSet::with_capacity(registers), ranges }
    }

    fn set(&mut self, register: Register, range: Option<IntRange>, current: bool) {
        self.ranges[usize::from(register)] = range;
        if current { self.current.insert(usize::from(register)); }
        else { self.current.remove(usize::from(register)); }
    }

    fn copy(&mut self, to: Register, from: Register) {
        self.set(to, self.ranges[usize::from(from)], self.current.contains(usize::from(from)));
    }

    fn write_field(&mut self, range: IntRange) {
        self.field = range;
        self.current.clear();
    }

    fn operand(&self, program: &ResidualProgram, operand: Operand) -> Option<IntRange> {
        match operand.kind()? {
            OperandKind::Register => self.ranges[usize::from(operand.payload())],
            OperandKind::Constant => program.constants.get(usize::from(operand.payload()))
                .and_then(super::super::facts::immediate).and_then(Value::as_int).map(IntRange::singleton),
            _ => None,
        }
    }

    fn refine(&mut self, operand: Operand, range: IntRange) -> Option<()> {
        if let Some(register) = operand.register_index() {
            self.ranges[usize::from(register)] = Some(range);
            if self.current.contains(usize::from(register)) {
                self.field = self.field.intersect(range)?;
                for alias in self.current.iter() {
                    self.ranges[alias] = Some(self.ranges[alias]?.intersect(self.field)?);
                }
            }
        }
        Some(())
    }

    fn join(&mut self, other: Self) {
        self.field = self.field.join(other.field);
        for (index, (known, incoming)) in self.ranges.iter_mut().zip(other.ranges).enumerate() {
            *known = known.zip(incoming).map(|(left, right)| left.join(right));
            if !other.current.contains(index) { self.current.remove(index); }
        }
    }

    /// The state on the edge of `instruction` that is taken where its comparison is `truth`;
    /// `None` where no value reaches the edge.
    fn branch(&mut self, program: &ResidualProgram, instruction: WideInstruction, truth: bool) -> Option<()> {
        if !matches!(crate::compiled::decoded(instruction), Some(quench_runtime::staged::Operation::CompareJump { .. })) { return Some(()); }
        let Some(relation) = NumberRelation::from_operator(instruction.binary_operator_field()) else { return Some(()) };
        let operands = [instruction.operand_b(), instruction.operand_c()];
        let edge = Edge::of(&|operand| self.operand(program, operand), relation, truth, operands);
        match edge {
            Edge::Unrefined => Some(()),
            Edge::Infeasible => None,
            Edge::Refined([left, right]) => { self.refine(operands[0], left)?; self.refine(operands[1], right) }
        }
    }

    /// The range of the value `instruction` defines.
    fn result(&self, program: &ResidualProgram, instruction: WideInstruction) -> Option<IntRange> {
        ranges::result(&|operand| self.operand(program, operand), &program.constants, instruction)
    }
}

/// The call marker at `pc` initializes the inlined callee's registers: its window is unknown
/// and each passed argument is a copy of the caller's register.
fn enter_frame(state: &mut State, program: &ResidualProgram, unit: &super::super::lane_unit::Unit, pc: usize) {
    let Some(frame) = unit.markers.iter().position(|marker| *marker == pc).map(|index| unit.frames[index]) else { return };
    for register in frame.base..frame.base + frame.registers { state.set(register, None, false); }
    for (to, from) in frame.passed_registers(program.function(frame.target.function as usize)) { state.copy(to, from); }
}

/// The field write `instruction` makes: its value's range, which must lie in the packed range
/// to be a proof; whether it does.
fn record_store(state: &mut State, instruction: WideInstruction, slot: &mut Option<IntRange>) -> bool {
    let Some(range) = state.ranges[usize::from(instruction.register_a())]
        .filter(|range| IntRange::packed().contains(*range)) else { return false };
    *slot = Some(range);
    state.write_field(range);
    true
}

/// The registers' ranges after `instruction`: a field `read` yields the field's, a move keeps its
/// source's range and alias, any other writes what its operands prove.
fn define(state: &mut State, program: &ResidualProgram, instruction: WideInstruction, read: bool) {
    let copied = (matches!(crate::compiled::decoded(instruction), Some(quench_runtime::staged::Operation::Move { .. })) && instruction.result_flags() == 0)
        .then(|| (state.ranges[usize::from(instruction.register_b())], state.current.contains(usize::from(instruction.register_b()))));
    let range = if read { Some(state.field) } else { state.result(program, instruction) };
    for register in writes(instruction) { state.set(register, None, false); }
    if read || range.is_some() { state.set(instruction.result_register(), range, read); }
    if let Some((range, current)) = copied { state.set(instruction.result_register(), range, current); }
}

/// The state `state` after the instruction at `pc`, joined into each forward successor within
/// the loop that its edge reaches.
fn forward(incoming: &mut [Option<State>], state: State, program: &ResidualProgram,
    (pc, instruction): (usize, WideInstruction), (header, back): (usize, usize)) {
    let mut targets = successors(pc, instruction).into_iter().enumerate()
        .filter_map(|(edge, target)| (target > pc && target <= back).then_some((edge, target))).peekable();
    let mut state = Some(state);
    while let Some((edge, target)) = targets.next() {
        let mut next = if targets.peek().is_some() { state.as_ref().unwrap().clone() } else { state.take().unwrap() };
        if next.branch(program, instruction, edge == 0).is_none() { continue; }
        let slot = &mut incoming[target - header];
        if let Some(known) = slot { known.join(next); } else { *slot = Some(next); }
    }
}

pub(super) fn analyze(program: &ResidualProgram, unit: &super::super::lane_unit::Unit,
    field: &LoopField, back: usize) -> smallvec::SmallVec<[Option<IntRange>; INLINE_FIELD_BINDINGS]> {
    let mut stores: smallvec::SmallVec<[Option<IntRange>; INLINE_FIELD_BINDINGS]> = smallvec::smallvec![None; field.stores.len()];
    let mut incoming = vec![None; back - field.header + 1];
    incoming[0] = Some(State::new(vec![None; usize::from(unit.registers)]));
    for pc in field.header..=back {
        let Some(mut state) = incoming[pc - field.header].take() else { continue };
        enter_frame(&mut state, program, unit, pc);
        let instruction = unit.instructions[pc];
        if let Some(index) = field.stores.iter().position(|store| *store == pc)
            && !record_store(&mut state, instruction, &mut stores[index]) {
            stores.fill(None);
            return stores;
        }
        define(&mut state, program, instruction, field.reads.contains(&pc));
        forward(&mut incoming, state, program, (pc, instruction), (field.header, back));
    }
    // One inductive packed-field invariant authorizes all of its store proofs.
    // An unproven write cannot make a later read look safe by assumption.
    if stores.iter().any(|range| !range.is_some_and(|range| IntRange::packed().contains(range))) { stores.fill(None); }
    stores
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn field_writes_retire_equality_but_preserve_numeric_snapshots() {
        let old = IntRange { min: 1, max: 7 };
        let mut state = State::new(vec![Some(old); 2]);
        state.field = old;
        state.current.insert(0);
        state.copy(1, 0);
        state.write_field(IntRange::singleton(0));
        assert!(state.current.iter().next().is_none());
        assert_eq!(state.ranges, vec![Some(old); 2]);
        assert_eq!(state.field, IntRange::singleton(0));
        let mut other = state.clone();
        other.set(0, Some(IntRange::singleton(-1)), false);
        state.join(other);
        assert_eq!(state.ranges[0], Some(IntRange { min: -1, max: 7 }));
        assert_eq!(state.field, IntRange::singleton(0));
    }
}
