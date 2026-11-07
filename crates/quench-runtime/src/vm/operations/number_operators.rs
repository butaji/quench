//! The Number operator table: which binary operators are numeric, and what each computes on
//! two Numbers. The interpreter, its native emitters and the compiler's static fold all read
//! these definitions, so a constant folded at parse is the value the interpreter computes.

use super::*;

/// A comparison operator applied to two numbers, or to two integers, which compare as the
/// numbers they denote.
#[inline(always)]
pub(super) fn number_comparison<T: PartialOrd>(op: u32, a: T, b: T) -> bool {
    NumberRelation::from_operator(op)
        .expect("comparison operators end at GreaterEqualThan")
        .apply(&mut PartialOrderStage, a, b)
}

pub(super) const FIRST_NUMERIC_BINARY_OPERATOR: u32 = BinaryOperator::Addition as u32;
pub(super) const LAST_NUMERIC_BINARY_OPERATOR: u32 = BinaryOperator::BitwiseAnd as u32;
pub(super) const NUMERIC_BINARY_OPERATOR_COUNT: usize =
    (LAST_NUMERIC_BINARY_OPERATOR - FIRST_NUMERIC_BINARY_OPERATOR + 1) as usize;
pub(super) const NUMERIC_BINARY_OPERATORS: [BinaryOperator; NUMERIC_BINARY_OPERATOR_COUNT] = [
    BinaryOperator::Addition,
    BinaryOperator::Subtraction,
    BinaryOperator::Multiplication,
    BinaryOperator::Division,
    BinaryOperator::Remainder,
    BinaryOperator::Exponential,
    BinaryOperator::ShiftLeft,
    BinaryOperator::ShiftRight,
    BinaryOperator::ShiftRightZeroFill,
    BinaryOperator::BitwiseOR,
    BinaryOperator::BitwiseXOR,
    BinaryOperator::BitwiseAnd,
];

#[inline(always)]
pub(super) fn numeric_binary_operator(immediate: u32) -> Option<BinaryOperator> {
    let index = immediate.checked_sub(FIRST_NUMERIC_BINARY_OPERATOR)? as usize;
    let operator = *NUMERIC_BINARY_OPERATORS.get(index)?;
    (operator as u32 == immediate).then_some(operator)
}

#[inline(always)]
pub(super) fn is_numeric_binary_operator(immediate: u32) -> bool {
    (FIRST_NUMERIC_BINARY_OPERATOR..=LAST_NUMERIC_BINARY_OPERATOR).contains(&immediate)
}

#[inline(always)]
pub(super) fn numeric_number_result(operator: BinaryOperator, left: f64, right: f64) -> f64 {
    if let Some(operation) = NumberArithmetic::from_operator(operator as u32) {
        return number_arithmetic_result(operation, left, right);
    }
    if let Some(operation) = NumberBitwise::from_operator(operator as u32) {
        let left = number_to_u32(left) as i32;
        let right = number_to_u32(right) as i32;
        return number_bitwise_f64_result(operation, left, right);
    }
    match operator {
        BinaryOperator::Remainder => number_remainder(left, right),
        BinaryOperator::Exponential => exponentiate(left, right),
        _ => unreachable!("numeric operator table only contains numeric operators"),
    }
}

/// Operator `op` on two Numbers known before run time, as the interpreter computes it: a
/// comparison's boolean, or a numeric operator's Number; `None` for any other operator. The
/// compiler folds static operands through this (`compile::binding_time`).
pub(crate) fn static_number_binary(op: u32, left: f64, right: f64) -> Option<Constant> {
    if op <= LAST_COMPARISON_OPERATOR {
        return Some(Constant::Boolean(number_comparison(op, left, right)));
    }
    Some(Constant::Number(numeric_number_result(numeric_binary_operator(op)?, left, right)))
}
