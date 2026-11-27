//! The result types of the numeric and string operators, of an increment and of a relation's edges, over the operands'
//! types. This is the one transfer the solve applies to `Binary`, `IncDec` and the fused Number operations
//! (`effects::value_of`, `effects::number_value`). The integer rules are `typed_ir::IntRange`'s (`int_range.rs`); this
//! file maps each operator to them.

use crate::typed_ir::{IntRange, Type};
use crate::ValueType;
use crate::vm::operations::{binary_operator as op, NumberArithmetic, NumberBitwise, NumberRelation};

/// The type `left operator right` defines, where a rule names the operator: a BigInt of two BigInts, a string of a `+`
/// with a string, else the Number of a numeric operator (a BigInt beside no Number). `None` for an operator no rule
/// names here: a comparison, `in`, `instanceof`, or a `+` whose operands may concatenate or be any value.
pub(super) fn binary_type(operator: u32, left: Type, right: Type) -> Option<Type> {
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

/// The type of an arithmetic operator over two Numbers: the integer range of two integer ranges (`int_arithmetic`),
/// else a Number.
pub(super) fn arithmetic(operation: NumberArithmetic, left: Type, right: Type) -> Type {
    match (left.int_range(), right.int_range()) {
        (Some(left), Some(right)) => crate::typed_ir::arithmetic::number_range(operation, left, right).map_or(Type::NUMBER, Type::int),
        _ => Type::NUMBER,
    }
}

/// The type of a bitwise operator over Numbers. An operand reads as its int32 (`int32_operand`), so every operator but
/// `>>>` defines an int32 whatever its operands; `>>>` defines an int32 only by a shift of nonzero count.
pub(super) fn bitwise(operation: NumberBitwise, left: Type, right: Type) -> Type {
    let operand = |ty: Type| ty.int_range().unwrap_or(IntRange::FULL).int32_operand();
    let (left, right) = (operand(left), operand(right));
    match operation {
        NumberBitwise::And => Type::int(left.and(right)),
        NumberBitwise::Or => Type::int(left.or(right)),
        NumberBitwise::Xor => Type::int(left.bitwise_hull(right)),
        // A left shift's word is ToInt32 of its result, which lies in int32 whatever its operands.
        NumberBitwise::ShiftLeft => Type::INT32,
        NumberBitwise::ShiftRight => Type::int(left.shift_right(right).unwrap_or(IntRange::FULL)),
        NumberBitwise::ShiftRightUnsigned => left.shift_right_unsigned(right).map_or(Type::NUMBER, Type::int),
    }
}

/// The type of `left % right` over Numbers: the remainder's range of two integer ranges (`IntRange::remainder`), else a
/// Number.
pub(super) fn remainder(left: Type, right: Type) -> Type {
    match (left.int_range(), right.int_range()) {
        (Some(left), Some(right)) => left.remainder(right).map_or(Type::NUMBER, Type::int),
        _ => Type::NUMBER,
    }
}

/// The type `++` (or `--`, where `decrement`, and also its local-slot form, which defines the same value) defines over an
/// operand of type `source`: the stepped range of an integer where it stays in int32 (an int32 then), else a Number,
/// or a Number or BigInt where the operand may be a BigInt.
pub(super) fn step(source: Type, decrement: bool) -> Type {
    if source == Type::NEVER { return Type::NEVER; }
    if let Some(range) = source.int_range() {
        return range.stepped(decrement).filter(IntRange::fits_int32).map_or(Type::NUMBER, Type::int);
    }
    if source.is_subtype(Type::NUMBER) { Type::NUMBER } else { Type::NUMBER.join(Type::language(ValueType::BigInt)) }
}

/// The type a backedge carries into a loop's header (`flow::Solver::transfer_edge_at`): an integer range within int32
/// widens to int32's whole span, and one beyond int32 to a Number. The header then joins finitely many types, so a
/// counter a loop steps settles: an int32 while its stepped range stays in int32 (`step`), else a Number.
pub(super) fn settle(ty: Type) -> Type {
    if ty.is_int32() { Type::INT32 } else if ty.int_range().is_some() { Type::NUMBER } else { ty }
}

/// The ranges a relational jump proves of its operands `left` and `right` on the edge taken where the relation holds
/// (`truth`), or where it fails. `None` where no pair of values reaches the edge.
pub(super) fn relation_edge(relation: NumberRelation, truth: bool, left: IntRange, right: IntRange) -> Option<[IntRange; 2]> {
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
