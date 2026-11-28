//! Decoding a binary operator and its constants into a `NumberOperation`.

use super::*;

/// A constant operand's int32 and Number readings (`NumericFrame::constant_operand`). The stencil
/// build holds no constant table, so there no constant reaches them.
#[cfg(not(stencil_build))]
#[inline(always)]
fn constant_int(value: crate::Value) -> Option<i32> { value.as_int() }
#[cfg(stencil_build)]
#[inline(always)]
fn constant_int(_value: crate::Value) -> Option<i32> { None }
#[cfg(not(stencil_build))]
#[inline(always)]
pub(super) fn constant_number(value: crate::Value) -> Option<f64> { value.as_number() }
#[cfg(stencil_build)]
#[inline(always)]
pub(super) fn constant_number(_value: crate::Value) -> Option<f64> { None }

/// ToInt32 of a constant Number (`crate::value::number_to_u32`'s bits).
#[cfg(not(stencil_build))]
#[inline(always)]
fn constant_int32(number: f64) -> i32 { crate::value::number_to_u32(number) as i32 }
#[cfg(stencil_build)]
#[inline(always)]
fn constant_int32(_number: f64) -> i32 { 0 }

/// The Number operation a `Binary` of `operator` over `left` and `right` denotes, defining
/// `destination`: an arithmetic, a remainder, a power, a bitwise or a relation. The decode cannot see the
/// operands' representations, so it names the widest domain (`Float64`; a bitwise is int32 by its
/// operator). `None` for any other operator (equality, `in`, `instanceof`): the VM's. The operator
/// tables are the operations' own (`NumberArithmetic::from_operator` and its siblings).
#[cfg(not(stencil_build))]
#[inline(always)]
pub fn binary_operation(operator: u32, destination: Register, left: Operand, right: Operand) -> Option<NumberOperation> {
    if let Some(operation) = NumberArithmetic::from_operator(operator) {
        return Some(NumberOperation::Arithmetic { destination, left, right, operation, domain: Domain::Float64,
            overflow: Overflow::Widen, zero: NegativeZero::Ignore });
    }
    if operator == binary_operator::Remainder {
        return Some(NumberOperation::Remainder { destination, left, right, domain: Domain::Float64,
            zero: NegativeZero::Exit, divisor_nonzero: false });
    }
    if operator == binary_operator::Exponential {
        return Some(NumberOperation::Power { destination, base: left, exponent: right });
    }
    if let Some(operation) = NumberBitwise::from_operator(operator) {
        return Some(NumberOperation::Bitwise { destination, left, right, operation });
    }
    NumberRelation::from_operator(operator).map(|relation|
        NumberOperation::RelationValue { destination, left, right, relation, domain: Domain::Float64 })
}

/// The Number `operator`'s `binary_operation` row (an arithmetic, the remainder or a bitwise) computes of two
/// proved binary64s, boxed as `Value::number` boxes it: a construct plan's computed field (`plan_fields`) takes
/// the operator from the Number step's own decode, not a table of its own. A relation, or an operator whose row is
/// no arithmetic, remainder or bitwise (`**`, a power, which has no word-only form here), leaves at `miss`.
#[cfg(not(stencil_build))]
#[inline(always)]
pub fn binary64_value<S: NumericStage>(stage: &mut S, operator: u32, left: super::super::NumberOf<S>, right: super::super::NumberOf<S>,
    miss: S::Miss) -> Option<S::Word> {
    let number = match binary_operation(operator, 0, Operand::register(0), Operand::register(1)) {
        Some(NumberOperation::Arithmetic { operation, .. }) => stage.float_arithmetic(operation, left.word(), right.word()),
        Some(NumberOperation::Remainder { .. }) => stage.float_remainder(left.word(), right.word()),
        Some(NumberOperation::Bitwise { operation, .. }) => {
            let (left, right) = (super::super::Int32::truncating(stage, left, miss)?, super::super::Int32::truncating(stage, right, miss)?);
            let bits = stage.int_bitwise(operation, left.word(), right.word());
            // A zero-fill shift's result is a uint32; every other bitwise result an int32.
            if operation == NumberBitwise::ShiftRightUnsigned { stage.u32_to_f64(bits) } else { stage.i32_to_f64(bits) }
        }
        _ => {
            let zero = stage.constant(0);
            let never = stage.ne_imm(zero, 0u64);
            stage.guard(never, miss)?;
            return None;
        }
    };
    Some(stage.box_number(super::super::Float64::of_row(number)))
}

/// `binary_operation` for two operands an int32 constant decides: add, subtract and multiply in
/// `Int32`, leaving at an overflow and at a negative zero, and the remainder in `Int32`, leaving at a
/// zero divisor and a negative zero. A division has no int32 result, and the other operators are
/// `binary_operation`'s own.
#[cfg(not(stencil_build))]
#[inline(always)]
pub fn int32_binary_operation(operator: u32, destination: Register, left: Operand, right: Operand) -> Option<NumberOperation> {
    let int32 = match NumberArithmetic::from_operator(operator) {
        Some(NumberArithmetic::Divide) => None,
        Some(operation) => Some(NumberOperation::Arithmetic { destination, left, right, operation, domain: Domain::Int32,
            overflow: Overflow::Exit, zero: NegativeZero::Exit }),
        None if operator == binary_operator::Remainder =>
            Some(NumberOperation::Remainder { destination, left, right, domain: Domain::Int32,
                zero: NegativeZero::Exit, divisor_nonzero: false }),
        None => None,
    };
    int32.or_else(|| binary_operation(operator, destination, left, right))
}
