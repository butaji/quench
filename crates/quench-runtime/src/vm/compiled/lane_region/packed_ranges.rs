//! Conditional numeric refinement of one admitted packed field. The loop's
//! existing alias plan owns object identity; this analysis owns numeric bounds.
use super::*;
use crate::vm::operations::{NumberArithmetic, NumberBitwise, NumberRelation};

/// What ranges cannot say: a register's relation to an array's `length`, naming the
/// register that holds the array. A relation dies with either register's next write.
#[derive(Clone)]
pub(super) struct Relations {
    /// Registers holding the `length` of the named register's array.
    pub(super) length_of: Vec<Option<Register>>,
    /// Registers known below the `length` of the named register's array.
    pub(super) below: Vec<Option<Register>>,
}

impl Relations {
    fn new(registers: usize) -> Self { Self { length_of: vec![None; registers], below: vec![None; registers] } }

    fn kill(&mut self, register: Register) {
        for slot in self.length_of.iter_mut().chain(self.below.iter_mut()) {
            if *slot == Some(register) { *slot = None; }
        }
        self.length_of[usize::from(register)] = None;
        self.below[usize::from(register)] = None;
    }

    pub(super) fn clear(&mut self) {
        self.length_of.fill(None);
        self.below.fill(None);
    }

    fn join(&mut self, other: &Self) {
        let pairs = self.length_of.iter_mut().zip(&other.length_of).chain(self.below.iter_mut().zip(&other.below));
        for (known, incoming) in pairs {
            if known != incoming { *known = None; }
        }
    }
}

#[derive(Clone)]
pub(super) struct State {
    field: IntRange,
    pub(super) ranges: Vec<Option<IntRange>>,
    pub(super) relations: Relations,
    /// Registers equal to the current field, invalidated by every field write.
    current: CompoundBitSet,
}

impl State {
    pub(super) fn new(ranges: Vec<Option<IntRange>>) -> Self {
        let registers = ranges.len();
        Self { field: IntRange::packed(), relations: Relations::new(registers),
            current: CompoundBitSet::with_capacity(registers), ranges }
    }

    pub(super) fn set(&mut self, register: Register, range: Option<IntRange>, current: bool) {
        self.relations.kill(register);
        self.ranges[usize::from(register)] = range;
        if current { self.current.insert(usize::from(register)); }
        else { self.current.remove(usize::from(register)); }
    }

    pub(super) fn copy(&mut self, to: Register, from: Register) {
        let (length, below) = (self.relations.length_of[usize::from(from)], self.relations.below[usize::from(from)]);
        self.set(to, self.ranges[usize::from(from)], self.current.contains(usize::from(from)));
        self.relations.length_of[usize::from(to)] = length.filter(|array| *array != to);
        self.relations.below[usize::from(to)] = below.filter(|array| *array != to);
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

    pub(super) fn join(&mut self, other: Self) {
        self.field = self.field.join(other.field);
        self.relations.join(&other.relations);
        for (index, (known, incoming)) in self.ranges.iter_mut().zip(other.ranges).enumerate() {
            *known = known.zip(incoming).map(|(left, right)| left.join(right));
            if !other.current.contains(index) { self.current.remove(index); }
        }
    }

    /// `small < big` holds, `big` holding an array's `length`: `small` is an index of it.
    fn bound_below_length(&mut self, [small, big]: [Operand; 2]) {
        if let Some((index, length)) = small.register_index().zip(big.register_index())
            && let Some(array) = self.relations.length_of[usize::from(length)]
            && array != index {
            self.relations.below[usize::from(index)] = Some(array);
        }
    }

    pub(super) fn branch(&mut self, program: &ResidualProgram, instruction: WideInstruction, truth: bool) -> Option<()> {
        if !instruction.op().is_jump_binary_false() { return Some(()); }
        let Some(relation) = NumberRelation::from_operator(instruction.binary_operator_field()) else { return Some(()) };
        let operands = [instruction.operand_b(), instruction.operand_c()];
        match effective_relation(relation, truth) {
            NumberRelation::LessThan => self.bound_below_length(operands),
            NumberRelation::GreaterThan => self.bound_below_length([operands[1], operands[0]]),
            _ => {}
        }
        let Some((left, right)) = self.operand(program, operands[0]).zip(self.operand(program, operands[1])) else { return Some(()) };
        let (left, right) = relation_ranges(relation, truth, left, right)?;
        self.refine(operands[0], left)?;
        self.refine(operands[1], right)
    }
}

/// The relation an edge proves of its operands: `relation` where `truth`, else its negation.
fn effective_relation(relation: NumberRelation, truth: bool) -> NumberRelation {
    use NumberRelation::*;
    if truth { return relation; }
    match relation {
        Equal => NotEqual, NotEqual => Equal, LessThan => GreaterEqual,
        LessEqual => GreaterThan, GreaterThan => LessEqual, GreaterEqual => LessThan,
    }
}

fn relation_ranges(relation: NumberRelation, truth: bool, left: IntRange, right: IntRange) -> Option<(IntRange, IntRange)> {
    use NumberRelation::*;
    let relation = effective_relation(relation, truth);
    match relation {
        Equal => { let common = left.intersect(right)?; Some((common, common)) }
        NotEqual => (left != right || left.min != left.max).then_some((left, right)),
        GreaterThan | GreaterEqual => relation_ranges(
            if relation == GreaterThan { LessThan } else { LessEqual }, true, right, left).map(|(right, left)| (left, right)),
        LessThan | LessEqual => {
            let strict = i64::from(relation == LessThan);
            Some((IntRange::bounds(i64::from(left.min), i64::from(left.max).min(i64::from(right.max) - strict))?,
                IntRange::bounds(i64::from(right.min).max(i64::from(left.min) + strict), i64::from(right.max))?))
        }
    }
}

pub(super) fn result(state: &State, program: &ResidualProgram, instruction: WideInstruction) -> Option<IntRange> {
    if instruction.result_flags() != 0 { return None; }
    if instruction.op().is_load_const() {
        return program.constants.get(instruction.constant_index())
            .and_then(super::super::facts::immediate).and_then(Value::as_int).map(IntRange::singleton);
    }
    if instruction.op().class().eq(Class::IncDec) { return step_result(state, instruction); }
    if instruction.op().js_denotation() != crate::bytecode::JsDenotation::NumericBinary { return None; }
    let (left, right) = (state.operand(program, instruction.operand_b()), state.operand(program, instruction.operand_c()));
    if let Some(operation) = NumberArithmetic::from_js_instruction(instruction.op(), instruction.binary_operator()) {
        return arithmetic_result(operation, left, right);
    }
    NumberBitwise::from_js_instruction(instruction.op(), instruction.binary_operator())
        .and_then(|operation| bitwise_result(operation, left, right))
}

/// The range an increment or decrement of an int32 register leaves.
fn step_result(state: &State, instruction: WideInstruction) -> Option<IntRange> {
    if instruction.numeric_local_target().is_some() { return None; }
    let (source, one) = (state.ranges[usize::from(instruction.register_b())]?, IntRange::singleton(1));
    if instruction.boolean_flag()? { source.subtract(one) } else { source.add(one) }
}

/// The range of an arithmetic operation over operands of ranges `left` and `right`.
fn arithmetic_result(operation: NumberArithmetic, left: Option<IntRange>, right: Option<IntRange>) -> Option<IntRange> {
    match operation {
        NumberArithmetic::Add => left?.add(right?),
        NumberArithmetic::Subtract => left?.subtract(right?),
        NumberArithmetic::Multiply => left?.multiply(right?),
        _ => None,
    }
}

/// The range of a bitwise operation over operands of ranges `left` and `right`.
fn bitwise_result(operation: NumberBitwise, left: Option<IntRange>, right: Option<IntRange>) -> Option<IntRange> {
    match operation {
        NumberBitwise::Or if right == Some(IntRange::singleton(0)) => Some(left.unwrap_or(IntRange::FULL)),
        NumberBitwise::Or if left == Some(IntRange::singleton(0)) => Some(right.unwrap_or(IntRange::FULL)),
        NumberBitwise::And | NumberBitwise::Or | NumberBitwise::Xor =>
            Some(left.unwrap_or(IntRange::FULL).bitwise_hull(right.unwrap_or(IntRange::FULL))),
        NumberBitwise::ShiftRight => left.unwrap_or(IntRange::FULL).shift_right(right?),
        _ => None,
    }
}

pub(super) fn analyze(program: &ResidualProgram, unit: &super::super::lane_unit::Unit,
    field: &LoopField, back: usize) -> smallvec::SmallVec<[Option<IntRange>; INLINE_FIELD_BINDINGS]> {
    let mut stores: smallvec::SmallVec<[Option<IntRange>; INLINE_FIELD_BINDINGS]> = smallvec::smallvec![None; field.stores.len()];
    let mut incoming = vec![None; back - field.header + 1];
    incoming[0] = Some(State::new(vec![None; usize::from(unit.registers)]));
    for pc in field.header..=back {
        let Some(mut state) = incoming[pc - field.header].take() else { continue };
        if let Some(frame) = unit.markers.iter().position(|marker| *marker == pc).map(|index| unit.frames[index]) {
            for register in frame.base..frame.base + frame.registers { state.set(register, None, false); }
            for (to, from) in frame.passed_registers(program.function(frame.target.function as usize)) { state.copy(to, from); }
        }
        let instruction = unit.instructions[pc];
        if let Some(index) = field.stores.iter().position(|store| *store == pc) {
            let Some(range) = state.ranges[usize::from(instruction.register_a())]
                .filter(|range| IntRange::packed().contains(*range)) else {
                    stores.fill(None);
                    return stores;
                };
            stores[index] = Some(range);
            state.write_field(range);
        }
        let copied = (instruction.op().is_move() && instruction.result_flags() == 0)
            .then(|| (state.ranges[usize::from(instruction.register_b())], state.current.contains(usize::from(instruction.register_b()))));
        let read = field.reads.contains(&pc);
        let range = if read { Some(state.field) } else { result(&state, program, instruction) };
        for register in writes(instruction) { state.set(register, None, false); }
        if read || range.is_some() { state.set(instruction.result_register(), range, read); }
        if let Some((range, current)) = copied { state.set(instruction.result_register(), range, current); }
        let mut targets = successors(pc, instruction).into_iter().enumerate()
            .filter_map(|(edge, target)| target.filter(|target| *target > pc && *target <= back).map(|target| (edge, target))).peekable();
        let mut state = Some(state);
        while let Some((edge, target)) = targets.next() {
            let mut next = if targets.peek().is_some() { state.as_ref().unwrap().clone() } else { state.take().unwrap() };
            if next.branch(program, instruction, edge == 0).is_none() { continue; }
            let slot = &mut incoming[target - field.header];
            if let Some(known) = slot { known.join(next); } else { *slot = Some(next); }
        }
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

    #[test]
    fn relation_edges_overapproximate_all_signed_integer_outcomes() {
        use NumberRelation::*;
        for min in -3..=3 { for max in min..=3 { for other_min in -3..=3 { for other_max in other_min..=3 {
            let left = IntRange { min, max };
            let right = IntRange { min: other_min, max: other_max };
            for relation in [Equal, NotEqual, LessThan, LessEqual, GreaterThan, GreaterEqual] {
                for truth in [false, true] {
                    let refined = relation_ranges(relation, truth, left, right);
                    for x in min..=max { for y in other_min..=other_max {
                        let actual = match relation { Equal => x == y, NotEqual => x != y,
                            LessThan => x < y, LessEqual => x <= y, GreaterThan => x > y, GreaterEqual => x >= y };
                        if actual == truth {
                            let (left, right) = refined.expect("reachable edge");
                            assert!(left.contains(IntRange::singleton(x)) && right.contains(IntRange::singleton(y)));
                        }
                    }}
                }
            }
        }}}}
        assert!(relation_ranges(LessThan, true, IntRange::singleton(i32::MIN), IntRange::singleton(i32::MIN)).is_none());
        assert!(relation_ranges(GreaterThan, true, IntRange::singleton(i32::MAX), IntRange::singleton(i32::MAX)).is_none());
    }
}
