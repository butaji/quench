//! The integer-range domain of the facts lattice: how a range flows through int32 arithmetic, a
//! step, a bitwise operator and a branch edge. A range is a type (`Type::int`), so the facts
//! analysis computes it with every other fact and `lane_region::int_ranges` and `packed_ranges`
//! read what it proves; this file is the one meaning of a range.

#[allow(unused_imports)]
use crate::ext_traits::*;
use super::{immediate, operand_type, set};
use quench_runtime::bytecode::{Constant, Operand, WideInstruction};
use quench_runtime::typed_ir::{IntRange, Type};
use quench_runtime::staged::{DecodeContext, NumberOperation, Operation};
use quench_runtime::value::Value;
use quench_runtime::vm::operations::{NumberArithmetic, NumberBitwise, NumberRelation};

/// The range of an operand: a register's by its type, a constant's by its value.
pub(crate) type Lookup<'a> = &'a dyn Fn(Operand) -> Option<IntRange>;

/// The ranges of `types`' registers and of `constants`.
pub(crate) fn lookup<'a>(types: &'a [Type], constants: &'a [Constant]) -> impl Fn(Operand) -> Option<IntRange> + 'a {
    move |operand| operand_type(constants, operand, |register| types.get(usize::from(register)).copied().unwrap_or(Type::ANY)).int_range()
}

/// The exact range of an int32 `operation` over operands of ranges `left` and `right`: it may
/// leave int32 (`IntRange::fits_int32`), which is where the operation overflows.
fn arithmetic(operation: NumberArithmetic, left: IntRange, right: IntRange) -> Option<IntRange> {
    match operation {
        NumberArithmetic::Add => left.add(right),
        NumberArithmetic::Subtract => left.subtract(right),
        NumberArithmetic::Multiply => left.multiply(right),
        _ => None,
    }
}

/// Whether a product of operands in these ranges may be negative zero, which no integer type
/// holds: zero is a product of an operand that may be zero, and negative where the other may be.
fn negative_zero(operation: NumberArithmetic, left: IntRange, right: IntRange) -> bool {
    let zero = IntRange::singleton(0);
    operation == NumberArithmetic::Multiply
        && ((left.contains(zero) && right.min < 0) || (right.contains(zero) && left.min < 0))
}

/// The arithmetic operator `instruction` is and its operands' ranges, where both are known.
fn operation(lookup: Lookup<'_>, instruction: WideInstruction) -> Option<(NumberArithmetic, IntRange, IntRange)> {
    if !super::super::meaning_compiled::is_numeric_binary(instruction) { return None; }
    let operation = NumberArithmetic::from_js_instruction(instruction.op(), instruction.binary_operator())?;
    let [left, right] = [instruction.operand_b(), instruction.operand_c()].map(lookup);
    Some((operation, left?, right?))
}

/// The range an increment or decrement of an int32 register steps its operand's to.
fn stepped(lookup: Lookup<'_>, instruction: WideInstruction) -> Option<IntRange> {
    if instruction.numeric_local_target().is_some() { return None; }
    let source = lookup(Operand::register(instruction.register_b()))?;
    let one = IntRange::singleton(1);
    if instruction.boolean_flag()? { source.subtract(one) } else { source.add(one) }
}

/// The exact range of the int32 addition, subtraction, product or step of `instruction` over its
/// operands' ranges, `None` for any other instruction or an operand of no range. A result inside
/// int32 never overflows, whatever int32 operands reach it.
pub(crate) fn exact(lookup: Lookup<'_>, instruction: WideInstruction) -> Option<IntRange> {
    if instruction.result_flags() != 0 { return None; }
    if matches!(Operation::<u64, ()>::decode(instruction, DecodeContext::EMPTY), Some(Operation::Number(NumberOperation::IncDec { .. }))) { return stepped(lookup, instruction); }
    let (operation, left, right) = operation(lookup, instruction)?;
    arithmetic(operation, left, right)
}

/// The range of the binary64 Numbers an add, subtract or multiply of integers in ranges `left`
/// and `right` makes (`IntRange::rounded`), where it stays within `IntRange::WORD`.
fn word_range(operation: NumberArithmetic, left: IntRange, right: IntRange) -> Option<IntRange> {
    arithmetic(operation, left, right).map(IntRange::rounded).filter(|range| IntRange::WORD.contains(*range))
}

/// `word_range` where the result is no negative zero: an integer Number, which no integer type holds a `-0` of.
fn number_range(operation: NumberArithmetic, left: IntRange, right: IntRange) -> Option<IntRange> {
    word_range(operation, left, right).filter(|_| !negative_zero(operation, left, right))
}

/// The type of an arithmetic operator's result: its range (`number_range`), else a Number.
pub(crate) fn arithmetic_type(lookup: Lookup<'_>, instruction: WideInstruction) -> Type {
    let Some((operation, left, right)) = operation(lookup, instruction) else { return Type::NUMBER };
    number_range(operation, left, right).map_or(Type::NUMBER, Type::int)
}

/// The range of the result of the arithmetic operator at `instruction` where it leaves int32:
/// an integer Number a 64-bit word holds (`IntRange::WORD`), which an index is computed in. A
/// `signless` product (no consumer reads the sign of its zero) may be a negative zero, which the
/// word holds as zero.
pub(crate) fn index_range(lookup: Lookup<'_>, instruction: WideInstruction, signless: bool) -> Option<IntRange> {
    let range = if signless { signless_range(lookup, instruction) } else { checked_range(lookup, instruction) };
    range.filter(|range| !range.fits_int32())
}

/// The range of the result of the arithmetic operator at `instruction`, where it is an integer Number.
fn checked_range(lookup: Lookup<'_>, instruction: WideInstruction) -> Option<IntRange> {
    if instruction.result_flags() != 0 { return None; }
    let (operation, left, right) = operation(lookup, instruction)?;
    number_range(operation, left, right)
}

/// The range of the product at `instruction` of ranged operands, whether or not it may be a
/// negative zero: what a result no consumer reads the sign of is typed by.
pub(crate) fn signless_range(lookup: Lookup<'_>, instruction: WideInstruction) -> Option<IntRange> {
    if instruction.result_flags() != 0 { return None; }
    let (operation, left, right) = operation(lookup, instruction)?;
    (operation == NumberArithmetic::Multiply).then(|| word_range(operation, left, right)).flatten()
}

/// Whether the product at `instruction` of ranged operands is no negative zero.
pub(crate) fn zero_free(lookup: Lookup<'_>, instruction: WideInstruction) -> bool {
    operation(lookup, instruction).is_some_and(|(operation, left, right)| !negative_zero(operation, left, right))
}

/// The transfer of an int32 increment or decrement: its operand is proved int32 (keeping the
/// range it has within int32), and its result is the stepped range within int32: a step that
/// would leave int32 leaves the native code instead.
pub(crate) fn step(types: &mut [Type], instruction: WideInstruction) {
    let operand = instruction.register_b();
    let range = types.get(usize::from(operand)).and_then(|ty| ty.int_range())
        .and_then(|range| range.intersect(IntRange::FULL)).unwrap_or(IntRange::FULL);
    set(types, operand, Type::int(range));
    let result = stepped(&lookup(types, &[]), instruction).and_then(|range| range.intersect(IntRange::FULL));
    set(types, instruction.result_register(), Type::int(result.unwrap_or(IntRange::FULL)));
}

/// The range of the bitwise `operation` over `operands`: an operand outside int32 or of no
/// range is any int32 (the operation takes ToInt32 of it), a shift needs a known count.
pub(crate) fn bitwise(lookup: Lookup<'_>, operation: NumberBitwise, operands: [Operand; 2]) -> IntRange {
    let [left, right] = operands.map(|operand| lookup(operand).filter(IntRange::fits_int32).unwrap_or(IntRange::FULL));
    let zero = IntRange::singleton(0);
    match operation {
        NumberBitwise::Or if right == zero => left,
        NumberBitwise::Or if left == zero => right,
        // A non-negative operand clears the bits of the other, so the result is non-negative and at
        // most the smallest non-negative operand's maximum (`i & 255` lies in 0..=255).
        NumberBitwise::And if left.min >= 0 || right.min >= 0 => {
            let ceiling = [left, right].iter().filter(|range| range.min >= 0).map(|range| range.max).min().unwrap_or(0);
            IntRange::bounds(0, ceiling).unwrap_or(IntRange::FULL)
        }
        NumberBitwise::And | NumberBitwise::Or | NumberBitwise::Xor => left.bitwise_hull(right),
        NumberBitwise::ShiftRight => left.shift_right(right).unwrap_or(IntRange::FULL),
        NumberBitwise::ShiftRightUnsigned => left.shift_right_unsigned(right).unwrap_or(IntRange::FULL),
        _ => IntRange::FULL,
    }
}

/// The range of the value `instruction` defines over its operands' ranges, for a consumer that
/// tracks ranges alone (`packed_ranges`): an int32 constant, the int32 result of an arithmetic
/// operation or a step that fits int32, a bitwise result; `None` for any other.
pub(crate) fn result(lookup: Lookup<'_>, constants: &[Constant], instruction: WideInstruction) -> Option<IntRange> {
    if instruction.result_flags() != 0 { return None; }
    if quench_runtime::staged::loads_constant(instruction.op()) {
        return constants.get(instruction.constant_index()).and_then(immediate).and_then(Value::as_int).map(IntRange::singleton);
    }
    let bitwise_operation = super::super::meaning_compiled::is_numeric_binary(instruction)
        .then(|| NumberBitwise::from_js_instruction(instruction.op(), instruction.binary_operator())).flatten();
    match bitwise_operation {
        Some(operation @ (NumberBitwise::And | NumberBitwise::Or | NumberBitwise::Xor | NumberBitwise::ShiftRight)) =>
            Some(bitwise(lookup, operation, [instruction.operand_b(), instruction.operand_c()])),
        // A zero-fill shift by a known nonzero count is an int32 (`NumberBitwise::result_is_int32`).
        Some(operation @ NumberBitwise::ShiftRightUnsigned) if lookup(instruction.operand_c()).is_some_and(|count| IntRange::FULL.shift_right_unsigned(count).is_some()) =>
            Some(bitwise(lookup, operation, [instruction.operand_b(), instruction.operand_c()])),
        Some(_) => None,
        None => exact(lookup, instruction).filter(IntRange::fits_int32),
    }
}

/// The relation an edge proves of its operands: `relation` where `truth`, else its negation.
pub(crate) fn effective_relation(relation: NumberRelation, truth: bool) -> NumberRelation {
    use NumberRelation::*;
    if truth { return relation; }
    match relation {
        Equal => NotEqual, NotEqual => Equal, LessThan => GreaterEqual,
        LessEqual => GreaterThan, GreaterThan => LessEqual, GreaterEqual => LessThan,
    }
}

/// `range` without the value `other` names when `other` is a singleton, which an interval can
/// drop only at its end: `range` ≠ v with `range` = [v, max] is [v + 1, max]. `None` where no
/// value is left (`range` is the singleton v).
fn without(range: IntRange, other: IntRange) -> Option<IntRange> {
    if other.min != other.max { return Some(range); }
    let value = other.min;
    if range.min == value && range.max == value { return None; }
    if range.min == value { IntRange::bounds(value + 1, range.max) }
    else if range.max == value { IntRange::bounds(range.min, value - 1) }
    else { Some(range) }
}

/// The ranges of `left` and `right` on an edge where `relation` holds if `truth`; `None` where
/// no pair of values reaches the edge.
fn relation_ranges(relation: NumberRelation, truth: bool, left: IntRange, right: IntRange) -> Option<(IntRange, IntRange)> {
    use NumberRelation::*;
    let relation = effective_relation(relation, truth);
    match relation {
        Equal => { let common = left.intersect(right)?; Some((common, common)) }
        NotEqual => Some((without(left, right)?, without(right, left)?)),
        GreaterThan | GreaterEqual => relation_ranges(
            if relation == GreaterThan { LessThan } else { LessEqual }, true, right, left).map(|(right, left)| (left, right)),
        LessThan | LessEqual => {
            let strict = i64::from(relation == LessThan);
            Some((IntRange::bounds(left.min, left.max.min(right.max - strict))?,
                IntRange::bounds(right.min.max(left.min + strict), right.max)?))
        }
    }
}

/// What a branch edge proves of its operands' ranges.
pub(crate) enum Edge {
    /// Nothing: an operand has no range.
    Unrefined,
    /// The operands' ranges there.
    Refined([IntRange; 2]),
    /// No values reach the edge.
    Infeasible,
}

impl Edge {
    /// The edge of a comparison by `relation` of `operands`, taken where `truth`.
    pub(crate) fn of(lookup: Lookup<'_>, relation: NumberRelation, truth: bool, operands: [Operand; 2]) -> Self {
        let [Some(left), Some(right)] = operands.map(lookup) else { return Self::Unrefined };
        relation_ranges(relation, truth, left, right).map_or(Self::Infeasible, |(left, right)| Self::Refined([left, right]))
    }
}

#[cfg(test)]
#[path = "ranges_tests.rs"] pub(crate) mod tests;
