//! The one typing of an operation: the cell a staged `Operation<(), ()>` defines over the cells of its operands, and the
//! numeric rules those cells follow (an operator's result, an increment, a relation's edges, a counter's settled type).
//! The solve (`compile/aot_facts`) applies these rules to every instruction, the JIT lane's graph lowering reads the same
//! cells through `Stage::fact`, and check deletion relies on them, so the table lives here beside the rows that decode
//! the operations, not in one consumer.
//!
//! The rules take and return `typed_ir` values only. What a value is bound to (its origin), the keys a step writes and
//! the walk over the instructions stay with the caller, which reads a rule's cell and keeps its own bookkeeping.

use super::math::FloatUnary;
use super::{NumberOperation, Operation};
use crate::bytecode::{Operand, OperandKind, Register};
use crate::typed_ir::{Cell, Elements, IntRange, Length, Type, language_bit};
use crate::vm::operations::{binary_operator as op, NumberArithmetic, NumberBitwise, NumberRelation, NumberUnary};
use crate::ValueType;

/// The cells an operation's operands read: a register's, a program constant's, and the elements no field names.
/// The solve implements it over the registers before an instruction; a lowering implements it over its own values.
pub trait Abstract {
    /// The cell of `register`.
    fn register(&self, register: Register) -> Cell;
    /// The cell of the program constant at `index`: the type of its value.
    fn constant(&self, index: usize) -> Cell;
    /// The elements of the arrays no field names hold, or `None` where the types are not solved yet, so an element
    /// read is any value.
    fn unnamed_elements(&self) -> Option<Elements>;
}

/// The cell of an operand: its register's, the type of its constant, else any value.
pub fn operand_cell(env: &impl Abstract, operand: Operand) -> Cell {
    match (operand.register_index(), operand.kind()) {
        (Some(register), _) => env.register(register),
        (None, Some(OperandKind::Constant)) => env.constant(usize::from(operand.payload())),
        _ => Cell::ANY,
    }
}

/// The cell an operation defines, where its value is typed by its operands: a Number, a `Binary`, a predicate's Boolean,
/// `void`'s `undefined`, an indexed read. `None` for the operations whose value keeps an operand's origin or binds a key
/// (moves, loads, closures, constants), which the caller types itself.
pub fn value(op: &Operation<(), ()>, env: &impl Abstract) -> Option<Cell> {
    Some(match *op {
        Operation::Number(number) => number_cell(env, number),
        Operation::Binary { left, right, operator, .. } => binary(env, left, right, operator),
        Operation::Equality { .. } | Operation::LogicalNot { .. } | Operation::TypeofTest { .. } | Operation::InstanceOf { .. } =>
            Cell::of(Type::language(ValueType::Boolean)),
        Operation::Void { .. } => Cell::of(Type::language(ValueType::Undefined)),
        Operation::ElementRead { base, key, .. } => element(env, env.register(base), operand_cell(env, key)),
        _ => return None,
    })
}

/// The cell `left operator right` defines: a comparison is a Boolean whatever its operands; otherwise the operator's type
/// over the operands' types (`binary_type`), else any value. An operand no value reaches yet (`NEVER`, the solve's bottom)
/// makes the result `NEVER` too, except for `in` and `instanceof`, which stay any value.
pub fn binary(env: &impl Abstract, left: Operand, right: Operand, operator: u32) -> Cell {
    let (left, right) = (operand_cell(env, left).ty, operand_cell(env, right).ty);
    if left == Type::NEVER || right == Type::NEVER {
        return if matches!(operator, op::In | op::Instanceof) { Cell::ANY } else { Cell::of(Type::NEVER) };
    }
    if matches!(operator, op::Equality | op::Inequality | op::StrictEquality | op::StrictInequality
        | op::LessThan | op::LessEqualThan | op::GreaterThan | op::GreaterEqualThan) {
        return Cell::of(Type::language(ValueType::Boolean));
    }
    binary_type(operator, left, right).map_or(Cell::ANY, Cell::of)
}

/// The cell a Number operation defines, when its operands are Numbers: an operand that is not one may run guest code or
/// yield a string, so the operation then defines any value.
pub fn number_cell(env: &impl Abstract, number: NumberOperation) -> Cell {
    let fact = |operand: Operand| operand_cell(env, operand);
    // An operand no value reaches yet (`Type::NEVER`) is a Number among the others: what reaches it later is joined in
    // a later pass, and an operation over it defines none yet.
    let numeric = |cell: Cell| cell.ty.is_subtype(Type::NUMBER);
    let both = |left: Operand, right: Operand| numeric(fact(left)) && numeric(fact(right));
    let one = |source: Operand| numeric(fact(source));
    let register = |source: Register| env.register(source);
    let reached = |cells: &[Cell]| cells.iter().all(|cell| cell.ty != Type::NEVER);
    let operands: Vec<Cell> = match number {
        NumberOperation::Arithmetic { left, right, .. } | NumberOperation::Remainder { left, right, .. } | NumberOperation::Extreme { left, right, .. }
        | NumberOperation::Bitwise { left, right, .. } | NumberOperation::Imul { left, right, .. } => vec![fact(left), fact(right)],
        NumberOperation::Unary { source, .. } | NumberOperation::Math { source, .. } | NumberOperation::BitwiseNot { source, .. } => vec![fact(source)],
        NumberOperation::IncDec { source, .. } | NumberOperation::ToNumeric { source, .. } => vec![register(source)],
        _ => Vec::new(),
    };
    if !reached(&operands) && operands.iter().all(|cell| numeric(*cell)) { return Cell::of(Type::NEVER); }
    let ty = |operand: Operand| fact(operand).ty;
    match number {
        NumberOperation::Arithmetic { left, right, operation, .. } if both(left, right) => Cell::of(arithmetic(operation, ty(left), ty(right))),
        NumberOperation::Remainder { left, right, .. } if both(left, right) => Cell::of(remainder(ty(left), ty(right))),
        NumberOperation::Extreme { left, right, maximum, .. } if both(left, right) => Cell::of(extreme(maximum, ty(left), ty(right))),
        NumberOperation::Bitwise { left, right, operation, .. } if both(left, right) => Cell::of(bitwise(operation, ty(left), ty(right))),
        NumberOperation::Imul { left, right, .. } if both(left, right) => Cell::of(Type::INT32),
        NumberOperation::Unary { source, operation, .. } if one(source) => Cell::of(unary(operation, ty(source))),
        NumberOperation::Math { source, operation, .. } if one(source) => Cell::of(math(operation, ty(source))),
        NumberOperation::BitwiseNot { source, .. } if one(source) => Cell::of(Type::INT32),
        NumberOperation::IncDec { source, decrement, .. } if numeric(register(source)) => Cell::of(step(register(source).ty, decrement)),
        NumberOperation::ToNumeric { source, .. } if numeric(register(source)) => register(source),
        _ => Cell::ANY,
    }
}

/// The cell of an indexed read of `base` at `index`: the elements `base` names (`Elements::element`), joined with those no
/// field names (`Abstract::unnamed_elements`), and with `undefined` unless the index is within a fixed length
/// (`within_length`), where a hole may not read. Any value where the types are not solved yet.
pub fn element(env: &impl Abstract, base: Cell, index: Cell) -> Cell {
    let Some(unnamed) = env.unnamed_elements() else { return Cell::ANY };
    let elements = base.elements.join(unnamed);
    let element = elements.element();
    if within_length(elements.length, index.ty) { element } else { element.join(Cell::of(Type::language(ValueType::Undefined))) }
}

/// Whether an index of type `index` stays within an array of length `length`: a fixed length, and an integer range inside
/// `[0, length)`.
pub fn within_length(length: Length, index: Type) -> bool {
    match (length, index.int_range()) {
        (Length::Fixed(count), Some(range)) => range.min >= 0 && range.max < i64::from(count),
        _ => false,
    }
}

/// Whether a cell is a Number the solve has reached: its type is a Number and not the solve's bottom. The `ToNumeric` of
/// such a cell defines that cell itself.
pub fn reached_number(cell: Cell) -> bool {
    cell.ty != Type::NEVER && cell.ty.is_subtype(Type::NUMBER)
}

/// The type `left operator right` defines, where a rule names the operator: a BigInt of two BigInts, a string of a `+`
/// with a string, else the Number of a numeric operator (a BigInt beside no Number). `None` for an operator no rule
/// names here: a comparison, `in`, `instanceof`, or a `+` whose operands may concatenate or be any value.
pub fn binary_type(operator: u32, left: Type, right: Type) -> Option<Type> {
    let bigint = Type::language(ValueType::BigInt);
    let string = Type::language(ValueType::String);
    let number = |ty: Type| ty.is_subtype(Type::NUMBER);
    if bigint_operator(operator) && left.is_subtype(bigint) && right.is_subtype(bigint) { return Some(bigint); }
    if operator == op::Addition {
        if left.is_subtype(string) || right.is_subtype(string) { return Some(string); }
        return (number(left) && number(right)).then(|| arithmetic(NumberArithmetic::Add, left, right));
    }
    // A Number operand makes the other's coercion a Number too: a BigInt beside it throws. With neither a Number, the
    // operator's Number may be a BigInt instead.
    let numeric = |ty: Type| if number(left) || number(right) { ty } else { ty.join(bigint) };
    if let Some(operation) = NumberArithmetic::from_operator(operator) {
        return Some(numeric(arithmetic(operation, left, right)));
    }
    if let Some(operation) = NumberBitwise::from_operator(operator) {
        return Some(numeric(bitwise(operation, left, right)));
    }
    (operator == op::Remainder).then(|| numeric(remainder(left, right)))
}

/// Whether two BigInts give a BigInt by `operator`: the arithmetic, bitwise and remainder operators but `>>>`.
fn bigint_operator(operator: u32) -> bool {
    (NumberArithmetic::from_operator(operator).is_some() || NumberBitwise::from_operator(operator).is_some() || operator == op::Remainder)
        && operator != op::ShiftRightZeroFill
}

/// The type of an arithmetic operator over two Numbers. Two integer ranges give the operator's integer range
/// (`typed_ir::arithmetic::number_range`) where it has one. Otherwise the result is an integer or a NaN-free Number
/// (`NOT_NAN_NUMBER`), except where NaN is possible:
/// - an operand that may be NaN, or an infinity of a NaN-free non-integer (`Infinity - Infinity`, `0 * Infinity`):
///   no float range is kept, so any operand that is not an integer may be infinite, and `+`, `-`, `*` of such operands
///   is `NUMBER`;
/// - `0 / 0`: a division whose dividend and divisor both may be zero. A dividend or a divisor that excludes zero gives
///   an infinity or a finite quotient, never NaN.
pub fn arithmetic(operation: NumberArithmetic, left: Type, right: Type) -> Type {
    match (left.int_range(), right.int_range()) {
        (Some(left), Some(right)) => {
            if operation == NumberArithmetic::Divide && left.contains(IntRange::ZERO) && right.contains(IntRange::ZERO) {
                return Type::NUMBER;
            }
            // An integer result is never NaN; `number_range` is `None` for a quotient (not an integer) and a product that
            // leaves the word span, which are NaN-free Numbers all the same.
            crate::typed_ir::arithmetic::number_range(operation, left, right).map_or(Type::NOT_NAN_NUMBER, Type::int)
        }
        _ => Type::NUMBER,
    }
}

/// The type of `Math.max` (`maximum`) or `Math.min` of two Numbers. Two integer ranges give the integer range of their
/// extremes. Otherwise the result is NaN wherever either operand is (`Math.max(NaN, 1)` is NaN), so a NaN-free Number
/// only where neither may be.
pub fn extreme(maximum: bool, left: Type, right: Type) -> Type {
    match (left.int_range(), right.int_range()) {
        (Some(left), Some(right)) => {
            let (min, max) = if maximum {
                (left.min.max(right.min), left.max.max(right.max))
            } else {
                (left.min.min(right.min), left.max.min(right.max))
            };
            IntRange::bounds(min, max).map_or(Type::NOT_NAN_NUMBER, Type::int)
        }
        _ if left.may_be_nan() || right.may_be_nan() => Type::NUMBER,
        _ => Type::NOT_NAN_NUMBER,
    }
}

/// The type of `+source` (`Plus`) or `-source` (`Negate`) over a Number of type `source`. A unary keeps a Number's NaN
/// bit, since NaN is its own negation. Negating an integer range gives an integer range only where it excludes zero:
/// `-0` is a Number no integer type holds.
pub fn unary(operation: NumberUnary, source: Type) -> Type {
    match (operation, source.int_range()) {
        (NumberUnary::Plus, _) => source,
        (NumberUnary::Negate, Some(range)) if !range.contains(IntRange::ZERO) =>
            IntRange::bounds(-range.max, -range.min).map_or(Type::NOT_NAN_NUMBER, Type::int),
        (NumberUnary::Negate, _) if source.may_be_nan() => Type::NUMBER,
        (NumberUnary::Negate, _) => Type::NOT_NAN_NUMBER,
    }
}

/// The type of the Math function `operation` over a Number of type `source`. Every function returns NaN for a NaN
/// argument, so a source that may be NaN gives `NUMBER`. `Math.sqrt` of a negative is NaN, and a Number of unknown sign
/// may be negative (no float range is kept), so `Math.sqrt` is NaN-free only over an integer range with no negative
/// bound. Every other function of a NaN-free source (rounding, absolute value, sign, `fround`) is NaN-free.
pub fn math(operation: FloatUnary, source: Type) -> Type {
    if source.may_be_nan() { return Type::NUMBER; }
    match operation {
        FloatUnary::Sqrt if !source.int_range().is_some_and(|range| range.min >= 0) => Type::NUMBER,
        _ => Type::NOT_NAN_NUMBER,
    }
}

/// The type `ToNumber` of a value of type `ty` defines. A Number is its own `ToNumber`, a Boolean or `null` converts to
/// 0 or 1, and neither is NaN. `undefined` converts to NaN, a string may hold no numeral, and an object's `valueOf` may
/// return NaN, so any other type defines a Number that may be NaN.
pub fn to_number(ty: Type) -> Type {
    if ty == Type::NEVER || ty.is_subtype(Type::NUMBER) { return ty; }
    let nan_free = Type::languages(language_bit(ValueType::Boolean) | language_bit(ValueType::Null));
    if ty.is_subtype(nan_free) { Type::NOT_NAN_NUMBER } else { Type::NUMBER }
}


/// The type of a bitwise operator over Numbers. An operand reads as its int32 (`ToInt32` maps NaN to 0), so every
/// operator defines an integer: an int32 for all but `>>>`, which defines an int32 only by a shift of nonzero count and
/// otherwise an integer beyond int32's span (a NaN-free Number).
pub fn bitwise(operation: NumberBitwise, left: Type, right: Type) -> Type {
    let operand = |ty: Type| ty.int_range().unwrap_or(IntRange::FULL).int32_operand();
    let (left, right) = (operand(left), operand(right));
    match operation {
        NumberBitwise::And => Type::int(left.and(right)),
        NumberBitwise::Or => Type::int(left.or(right)),
        NumberBitwise::Xor => Type::int(left.bitwise_hull(right)),
        // A left shift's word is ToInt32 of its result, which lies in int32 whatever its operands.
        NumberBitwise::ShiftLeft => Type::INT32,
        NumberBitwise::ShiftRight => Type::int(left.shift_right(right).unwrap_or(IntRange::FULL)),
        NumberBitwise::ShiftRightUnsigned => left.shift_right_unsigned(right).map_or(Type::NOT_NAN_NUMBER, Type::int),
    }
}

/// The type of `left % right` over Numbers. A zero divisor gives NaN (`x % 0`), so a divisor that may be zero leaves
/// a Number that may be NaN. Otherwise the remainder's integer range (`IntRange::remainder`), or a NaN-free Number where
/// the dividend may be a negative zero.
pub fn remainder(left: Type, right: Type) -> Type {
    match (left.int_range(), right.int_range()) {
        (Some(left), Some(right)) if !right.contains(IntRange::ZERO) => left.remainder(right).map_or(Type::NOT_NAN_NUMBER, Type::int),
        _ => Type::NUMBER,
    }
}

/// The type `++` (or `--`, where `decrement`, and also its local-slot form, which defines the same value) defines over an
/// operand of type `source`: the stepped range of an integer where it stays in int32 (an int32 then), else the integer
/// beyond int32 (a NaN-free Number). A Number keeps its NaN bit, and a BigInt operand may be a BigInt.
pub fn step(source: Type, decrement: bool) -> Type {
    if source == Type::NEVER { return Type::NEVER; }
    if let Some(range) = source.int_range() {
        return range.stepped(decrement).filter(IntRange::fits_int32).map_or(Type::NOT_NAN_NUMBER, Type::int);
    }
    if source.is_subtype(Type::NUMBER) {
        if source.may_be_nan() { Type::NUMBER } else { Type::NOT_NAN_NUMBER }
    } else {
        Type::NUMBER.join(Type::language(ValueType::BigInt))
    }
}

/// The integer bounds a loop header widens its ranges to (`widen`): int32's two bounds, and each constant a relational
/// jump compares a value against, one below and one above it, since a strict and a non-strict comparison step a counter
/// one past the constant. Sorted and distinct, so the nearest one above or below a bound is a search.
pub fn thresholds(bounds: impl IntoIterator<Item = i64>) -> Vec<i64> {
    let mut all: Vec<i64> = bounds.into_iter().flat_map(|bound| [bound - 1, bound, bound + 1])
        .chain([IntRange::FULL.min, IntRange::FULL.max]).collect();
    all.sort_unstable();
    all.dedup();
    all
}

/// The type a loop header holds once its join has changed `rounds` times past `WIDENING_ROUNDS` (`flow::Solver::join`):
/// `joined` (the join of `known` and the incoming type) with each integer bound that grew moved to its nearest threshold
/// (`thresholds`), so the header's ranges reach a fixed bound in finitely many joins and stay within the thresholds a
/// relational edge compares against. Types that are not both integer ranges keep their join.
pub fn widen(thresholds: &[i64], known: Type, joined: Type) -> Type {
    match (known.int_range(), joined.int_range()) {
        (Some(known), Some(joined)) => Type::int(widen_range(thresholds, known, joined)),
        _ => joined,
    }
}

/// `joined`, a hull of `known` and an incoming range, with a bound that `known` did not reach moved to the nearest
/// threshold beyond it, or to `IntRange::WORD`'s bound where no threshold lies beyond (a value a word holds exactly).
fn widen_range(thresholds: &[i64], known: IntRange, joined: IntRange) -> IntRange {
    let up = |value: i64| thresholds.iter().copied().find(|&bound| bound >= value).unwrap_or(IntRange::WORD.max);
    let down = |value: i64| thresholds.iter().rev().copied().find(|&bound| bound <= value).unwrap_or(IntRange::WORD.min);
    IntRange {
        min: if joined.min < known.min { down(joined.min) } else { joined.min },
        max: if joined.max > known.max { up(joined.max) } else { joined.max },
    }
}

/// The ranges a relational jump proves of its operands `left` and `right` on the edge taken where the relation holds
/// (`truth`), or where it fails. `None` where no pair of values reaches the edge.
pub fn relation_edge(relation: NumberRelation, truth: bool, left: IntRange, right: IntRange) -> Option<[IntRange; 2]> {
    let relation = if truth { relation } else { negated(relation) };
    match relation {
        NumberRelation::Equal => {
            let common = left.intersect(right)?;
            Some([common, common])
        }
        NumberRelation::NotEqual => Some([left.without(right)?, right.without(left)?]),
        NumberRelation::LessThan => left.ordered_edge(right, true),
        NumberRelation::LessEqual => left.ordered_edge(right, false),
        NumberRelation::GreaterThan => right.ordered_edge(left, true).map(|[right, left]| [left, right]),
        NumberRelation::GreaterEqual => right.ordered_edge(left, false).map(|[right, left]| [left, right]),
    }
}

/// The relation that holds where `relation` does not.
fn negated(relation: NumberRelation) -> NumberRelation {
    match relation {
        NumberRelation::Equal => NumberRelation::NotEqual,
        NumberRelation::NotEqual => NumberRelation::Equal,
        NumberRelation::LessThan => NumberRelation::GreaterEqual,
        NumberRelation::LessEqual => NumberRelation::GreaterThan,
        NumberRelation::GreaterThan => NumberRelation::LessEqual,
        NumberRelation::GreaterEqual => NumberRelation::LessThan,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn span(min: i64, max: i64) -> IntRange { IntRange::bounds(min, max).unwrap() }

    fn range(min: i64, max: i64) -> Type { Type::int(span(min, max)) }

    /// Registers 0, 1, ... hold the given cells; no constants, and no elements no field names.
    struct Registers(Vec<Cell>);

    impl Abstract for Registers {
        fn register(&self, register: Register) -> Cell { self.0[usize::from(register)] }
        fn constant(&self, _index: usize) -> Cell { Cell::ANY }
        fn unnamed_elements(&self) -> Option<Elements> { None }
    }

    #[test]
    fn int32_add_of_two_ranges_is_the_range_of_their_sums() {
        let (left, right) = (range(0, 10), range(5, 20));
        assert_eq!(binary_type(op::Addition, left, right), Some(range(5, 30)));
        assert!(range(5, 30).is_int32());
        let env = Registers(vec![Cell::of(left), Cell::of(right)]);
        let add = Operation::Binary { destination: 2, left: Operand::register(0), right: Operand::register(1), operator: op::Addition };
        assert_eq!(value(&add, &env), Some(Cell::of(range(5, 30))));
    }

    #[test]
    fn increment_and_decrement_step_a_range_inside_int32() {
        assert_eq!(step(range(0, 10), false), range(1, 11));
        assert_eq!(step(range(0, 10), true), range(-1, 9));
    }

    #[test]
    fn increment_past_int32_widens_to_an_integer_that_is_not_nan() {
        assert_eq!(step(Type::INT32, false), Type::NOT_NAN_NUMBER);
        assert_eq!(step(Type::INT32, true), Type::NOT_NAN_NUMBER);
        assert_eq!(step(range(i64::from(i32::MAX), i64::from(i32::MAX)), false), Type::NOT_NAN_NUMBER);
    }

    #[test]
    fn integer_arithmetic_is_never_nan_and_a_non_integer_operand_may_be() {
        let integers = range(0, 10);
        assert!(!arithmetic(NumberArithmetic::Add, integers, integers).may_be_nan());
        let huge = Type::int(IntRange::singleton(1 << 40));
        assert_eq!(arithmetic(NumberArithmetic::Multiply, huge, huge), Type::NOT_NAN_NUMBER, "an integer product beyond the word");
        assert_eq!(arithmetic(NumberArithmetic::Add, Type::NUMBER, integers), Type::NUMBER);
        assert_eq!(arithmetic(NumberArithmetic::Add, Type::NOT_NAN_NUMBER, integers), Type::NUMBER, "an infinity may be a non-integer operand");
        assert_eq!(arithmetic(NumberArithmetic::Multiply, Type::NOT_NAN_NUMBER, Type::NOT_NAN_NUMBER), Type::NUMBER, "0 * Infinity");
        assert_eq!(arithmetic(NumberArithmetic::Subtract, Type::NOT_NAN_NUMBER, Type::NOT_NAN_NUMBER), Type::NUMBER, "Infinity - Infinity");
    }

    #[test]
    fn division_is_nan_only_where_zero_over_zero_is_possible() {
        assert_eq!(arithmetic(NumberArithmetic::Divide, range(0, 0), range(0, 4)), Type::NUMBER, "0 / 0");
        assert_eq!(arithmetic(NumberArithmetic::Divide, range(1, 9), range(0, 4)), Type::NOT_NAN_NUMBER, "x / 0 is an infinity");
        assert_eq!(arithmetic(NumberArithmetic::Divide, range(0, 9), range(1, 4)), Type::NOT_NAN_NUMBER, "0 / x is zero");
        assert_eq!(arithmetic(NumberArithmetic::Divide, Type::NUMBER, range(1, 4)), Type::NUMBER);
    }

    #[test]
    fn remainder_by_a_divisor_that_may_be_zero_may_be_nan() {
        assert_eq!(remainder(range(0, 9), range(0, 4)), Type::NUMBER, "x % 0 is NaN");
        assert_eq!(remainder(range(0, 9), range(3, 4)), range(0, 3));
        assert_eq!(remainder(range(-9, 9), range(3, 4)), Type::NOT_NAN_NUMBER, "a negative dividend may be -0");
    }

    #[test]
    fn math_functions_are_nan_for_a_nan_argument_and_sqrt_for_a_negative_one() {
        assert_eq!(math(FloatUnary::Sqrt, range(0, 16)), Type::NOT_NAN_NUMBER);
        assert_eq!(math(FloatUnary::Sqrt, range(-1, 4)), Type::NUMBER);
        assert_eq!(math(FloatUnary::Sqrt, Type::NOT_NAN_NUMBER), Type::NUMBER, "a non-integer of unknown sign");
        assert_eq!(math(FloatUnary::Floor, Type::NOT_NAN_NUMBER), Type::NOT_NAN_NUMBER);
        assert_eq!(math(FloatUnary::Floor, Type::NUMBER), Type::NUMBER);
        assert_eq!(math(FloatUnary::Sqrt, Type::NUMBER), Type::NUMBER);
    }

    #[test]
    fn max_and_min_are_nan_only_where_an_operand_is() {
        assert_eq!(extreme(true, Type::NOT_NAN_NUMBER, range(0, 3)), Type::NOT_NAN_NUMBER);
        assert_eq!(extreme(true, Type::NUMBER, range(0, 3)), Type::NUMBER, "Math.max(NaN, 1)");
        assert_eq!(extreme(false, range(0, 3), range(5, 9)), range(0, 3));
        assert_eq!(extreme(true, range(0, 3), range(5, 9)), range(5, 9));
    }

    #[test]
    fn unary_negation_keeps_the_nan_bit_and_negates_only_ranges_without_zero() {
        assert_eq!(unary(NumberUnary::Negate, range(1, 5)), range(-5, -1));
        assert_eq!(unary(NumberUnary::Negate, range(0, 5)), Type::NOT_NAN_NUMBER, "-0");
        assert_eq!(unary(NumberUnary::Negate, Type::NUMBER), Type::NUMBER);
        assert_eq!(unary(NumberUnary::Plus, Type::NUMBER), Type::NUMBER);
        assert_eq!(step(Type::NOT_NAN_NUMBER, false), Type::NOT_NAN_NUMBER);
        assert_eq!(step(Type::NUMBER, false), Type::NUMBER);
    }

    #[test]
    fn to_number_is_nan_free_for_numbers_booleans_and_null_only() {
        assert_eq!(to_number(Type::NUMBER), Type::NUMBER);
        assert_eq!(to_number(range(0, 3)), range(0, 3));
        assert_eq!(to_number(Type::language(ValueType::Boolean)), Type::NOT_NAN_NUMBER);
        assert_eq!(to_number(Type::language(ValueType::Null)), Type::NOT_NAN_NUMBER);
        assert_eq!(to_number(Type::language(ValueType::Undefined)), Type::NUMBER, "undefined converts to NaN");
        assert_eq!(to_number(Type::language(ValueType::String)), Type::NUMBER);
    }

    #[test]
    fn the_nan_bit_joins_by_or_and_integers_are_nan_free() {
        assert_eq!(range(0, 3).join(Type::NOT_NAN_NUMBER), Type::NOT_NAN_NUMBER);
        assert_eq!(Type::NOT_NAN_NUMBER.join(Type::NUMBER), Type::NUMBER);
        assert!(range(0, 3).is_subtype(Type::NOT_NAN_NUMBER) && Type::NOT_NAN_NUMBER.is_subtype(Type::NUMBER));
        assert!(!Type::NUMBER.is_subtype(Type::NOT_NAN_NUMBER));
        assert!(!Type::NOT_NAN_NUMBER.may_be_nan() && Type::NUMBER.may_be_nan());
        assert!(!Type::NEVER.may_be_nan() && !Type::language(ValueType::Boolean).may_be_nan());
        assert_eq!(Type::NOT_NAN_NUMBER.machine(), Type::NUMBER);
    }

    #[test]
    fn a_relation_edge_refines_a_counter() {
        let (counter, limit) = (span(0, 100), span(10, 10));
        // `counter < 10` holds on the taken edge: the counter keeps [0, 9] and the limit is still 10.
        assert_eq!(relation_edge(NumberRelation::LessThan, true, counter, limit), Some([span(0, 9), span(10, 10)]));
        // Where it fails the counter is at least the limit.
        assert_eq!(relation_edge(NumberRelation::LessThan, false, counter, limit), Some([span(10, 100), span(10, 10)]));
    }

    /// One loop's solve over a single register: the header joins its backedge for `WIDENING_ROUNDS` rounds, then widens
    /// (as `flow::Solver::join` does), and one descending pass recomputes the header from its backedge without widening.
    fn solve_header(entry: Type, step_body: impl FnMut(Type) -> Type) -> Type {
        solve_with(&thresholds([]), entry, step_body)
    }

    /// `solve_header` widening to the given thresholds: a loop header's own (`thresholds([])`), or a field's, which the
    /// solve widens to the constants stored to and compared against it (`aot_facts::types::Types::widen`).
    fn solve_with(thresholds: &[i64], entry: Type, mut step_body: impl FnMut(Type) -> Type) -> Type {
        let mut header = entry;
        for round in 0..=u32::from(crate::typed_ir::WIDENING_ROUNDS) {
            let joined = header.join(step_body(header));
            header = if round >= u32::from(crate::typed_ir::WIDENING_ROUNDS) { widen(thresholds, header, joined) } else { joined };
        }
        entry.join(step_body(header))
    }

    #[test]
    fn a_field_counter_incremented_under_a_bound_stays_within_its_constants() {
        // `this.count` is stored 0 at construction and `this.count = this.count + 1` runs only where `this.count < 1000`:
        // the field's cell is the join of the stores, so the solve's thresholds are the constants it stores and compares.
        let bounds = thresholds([0, 1000]);
        let field = solve_with(&bounds, Type::int(IntRange::singleton(0)), |field| {
            let [counter, _] = relation_edge(NumberRelation::LessThan, true, field.int_range().unwrap(), span(1000, 1000)).unwrap();
            step(Type::int(counter), false)
        });
        assert_eq!(field, range(0, 1000));
    }

    #[test]
    fn a_grown_field_range_widens_to_the_nearest_constant_not_to_int32() {
        // A counter stored 1000 and grown past 6 by one per round widens to 999 (the constant beside it), not int32's span.
        let bounds = thresholds([1000]);
        assert_eq!(widen(&bounds, range(0, 6), range(0, 7)), range(0, 999));
        // Past every constant, the widening settles at int32's span (`thresholds` always holds it), not `WORD`.
        assert_eq!(widen(&bounds, range(0, 6), range(0, 2000)), range(0, i64::from(i32::MAX)));
    }

    #[test]
    fn a_counted_loop_counter_stays_within_its_bound() {
        // `for (i = 0; i < n; i++)` with n in [0, 100]: the body sees i below n, so the header's back edge carries [1, n].
        let (entry, n) = (span(0, 0), span(0, 100));
        let header = solve_header(Type::int(entry), |header| {
            let [counter, _] = relation_edge(NumberRelation::LessThan, true, header.int_range().unwrap(), n).unwrap();
            step(Type::int(counter), false)
        });
        assert_eq!(header, range(0, 100));
    }

    #[test]
    fn a_carry_recurrence_reaches_a_bounded_fixed_point_inside_int32() {
        // `c = (c + x) >>> 15` with x in [0, 2^30): the carry is at most 2^15 once the sum is shifted, and it never leaves int32.
        let (x, shift) = (span(0, (1 << 30) - 1), Type::int(IntRange::singleton(15)));
        let carry = solve_header(Type::int(IntRange::singleton(0)), |carry| {
            bitwise(NumberBitwise::ShiftRightUnsigned, arithmetic(NumberArithmetic::Add, carry, Type::int(x)), shift)
        });
        assert!(carry.is_int32(), "the carry stays an int32: {carry:?}");
        assert!(carry.int_range().unwrap().max <= 1 << 15 || carry == Type::INT32, "{carry:?}");
    }
}
