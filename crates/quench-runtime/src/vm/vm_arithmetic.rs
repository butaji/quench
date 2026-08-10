//! VM op execution helpers (arithmetic, unary, binary, call dispatch).
use crate::bigint;
use crate::intl::tolocale::value::{
    is_truthy, loose_equal, strict_equal, to_int32, to_number, to_string, type_of,
};
use crate::ops::Builtin;
use crate::value::Value;

use super::{read_register, write_value, VmError};

pub(crate) fn execute_unary(
    registers: &mut Vec<Value>,
    dst: u16,
    operator: crate::ops::UnaryOp,
    src: u16,
) -> Result<(), VmError> {
    use crate::ops::UnaryOp;
    let value = read_register(registers, src)?;
    let result = match operator {
        UnaryOp::Plus => unary_plus(&value)?,
        UnaryOp::Minus => {
            if let Some(s) = bigint_value(&value) {
                Value::BigInt(bigint::negate(s).map_err(bigint_error)?)
            } else {
                numeric_unary(&value, |n| -n)?
            }
        }
        UnaryOp::Not => Value::Boolean(!is_truthy(&value)),
        UnaryOp::Void => Value::Undefined,
        UnaryOp::Typeof => Value::String(type_of(&value).to_string()),
        UnaryOp::ToString => Value::String(to_string(Some(&value))),
        UnaryOp::Delete => Value::Boolean(true),
    };
    write_value(registers, dst, result);
    Ok(())
}

fn numeric_unary(value: &Value, transform: fn(f64) -> f64) -> Result<Value, VmError> {
    Ok(Value::Number(transform(to_number(Some(value)))))
}

pub(crate) fn execute_binary(
    registers: &mut Vec<Value>,
    dst: u16,
    operator: crate::ops::BinaryOp,
    lhs: u16,
    rhs: u16,
) -> Result<(), VmError> {
    let left = read_register(registers, lhs)?;
    let right = read_register(registers, rhs)?;
    write_value(registers, dst, evaluate_binary(&left, &right, operator)?);
    Ok(())
}

fn evaluate_binary(
    left: &Value,
    right: &Value,
    operator: crate::ops::BinaryOp,
) -> Result<Value, VmError> {
    use crate::ops::BinaryOp;
    if let Some(result) = special_binary(left, right, operator)? {
        return Ok(result);
    }
    Ok(match operator {
        BinaryOp::Add
        | BinaryOp::Subtract
        | BinaryOp::Multiply
        | BinaryOp::Divide
        | BinaryOp::Remainder
        | BinaryOp::Exponentiate => arithmetic_value(left, right, operator),
        BinaryOp::Equal => Value::Boolean(loose_equal(left, right)),
        BinaryOp::NotEqual => Value::Boolean(!loose_equal(left, right)),
        BinaryOp::StrictEqual => Value::Boolean(strict_equal(left, right)),
        BinaryOp::StrictNotEqual => Value::Boolean(!strict_equal(left, right)),
        BinaryOp::LessThan => compare_values(left, right, |a, b| a < b)?,
        BinaryOp::LessEqual => compare_values(left, right, |a, b| a <= b)?,
        BinaryOp::GreaterThan => compare_values(left, right, |a, b| a > b)?,
        BinaryOp::GreaterEqual => compare_values(left, right, |a, b| a >= b)?,
        BinaryOp::BitwiseOr => bitwise_numbers(left, right, |a, b| a | b)?,
        BinaryOp::BitwiseXor => bitwise_numbers(left, right, |a, b| a ^ b)?,
        BinaryOp::BitwiseAnd => bitwise_numbers(left, right, |a, b| a & b)?,
        BinaryOp::ShiftLeft => bitwise_numbers(left, right, |a, b| a.wrapping_shl(shift_count(b)))?,
        BinaryOp::ShiftRight => {
            bitwise_numbers(left, right, |a, b| a.wrapping_shr(shift_count(b)))?
        }
        BinaryOp::ShiftRightZeroFill => Value::Number(shift_right_unsigned(left, right)),
        BinaryOp::Instanceof => Value::Boolean(instanceof(left, right)),
    })
}

fn instanceof(value: &Value, constructor: &Value) -> bool {
    let Value::Object(properties) = value else {
        return false;
    };
    properties
        .iter()
        .rev()
        .find(|(name, _)| name == "constructor")
        .is_some_and(|(_, value)| crate::builtins::same_value(Some(value), Some(constructor)))
}

fn special_binary(
    left: &Value,
    right: &Value,
    operator: crate::ops::BinaryOp,
) -> Result<Option<Value>, VmError> {
    use crate::ops::BinaryOp;
    if operator == BinaryOp::Add && has_string_operand(left, right) {
        return Ok(Some(arithmetic_value(left, right, operator)));
    }
    if is_bigint_arithmetic(operator) && has_bigint_operand(left, right) {
        return bigint_binary(left, right, operator).map(Some);
    }
    Ok(None)
}

fn bigint_binary(
    left: &Value,
    right: &Value,
    operator: crate::ops::BinaryOp,
) -> Result<Value, VmError> {
    use crate::ops::BinaryOp;
    let (Some(left_s), Some(right_s)) = (bigint_value(left), bigint_value(right)) else {
        return Err(type_error("Cannot mix BigInt and other types"));
    };
    Ok(match operator {
        BinaryOp::Add => Value::BigInt(bigint::add(left_s, right_s).map_err(bigint_error)?),
        BinaryOp::Subtract => {
            Value::BigInt(bigint::subtract(left_s, right_s).map_err(bigint_error)?)
        }
        BinaryOp::Multiply => {
            Value::BigInt(bigint::multiply(left_s, right_s).map_err(bigint_error)?)
        }
        BinaryOp::Divide => Value::BigInt(bigint::divide(left_s, right_s).map_err(bigint_error)?),
        BinaryOp::Remainder => {
            Value::BigInt(bigint::remainder(left_s, right_s).map_err(bigint_error)?)
        }
        BinaryOp::Exponentiate => {
            Value::BigInt(bigint::exponentiate(left_s, right_s).map_err(bigint_error)?)
        }
        _ => Value::Undefined,
    })
}

fn arithmetic_value(left: &Value, right: &Value, operator: crate::ops::BinaryOp) -> Value {
    if operator == crate::ops::BinaryOp::Add
        && (matches!(left, Value::String(_)) || matches!(right, Value::String(_)))
    {
        return Value::String(format!("{}{}", add_string(left), add_string(right)));
    }
    let left = to_number(Some(left));
    let right = to_number(Some(right));
    Value::Number(numeric_binary(left, right, operator))
}

fn bitwise_numbers(
    left: &Value,
    right: &Value,
    operation: fn(i32, i32) -> i32,
) -> Result<Value, VmError> {
    let left = to_int32(to_number(Some(left)));
    let right = to_int32(to_number(Some(right)));
    Ok(Value::Number(f64::from(operation(left, right))))
}

/// Shift amount: the low 5 bits of the right operand, per ECMAScript.
fn shift_count(right: i32) -> u32 {
    (right as u32) & 31
}

/// ECMAScript unsigned right shift: ToUint32(left) >> (count & 31), as a number.
fn shift_right_unsigned(left: &Value, right: &Value) -> f64 {
    let left = to_int32(to_number(Some(left))) as u32;
    let right = to_int32(to_number(Some(right)));
    f64::from(left >> shift_count(right))
}

fn numeric_binary(left: f64, right: f64, operator: crate::ops::BinaryOp) -> f64 {
    match operator {
        crate::ops::BinaryOp::Add => left + right,
        crate::ops::BinaryOp::Subtract => left - right,
        crate::ops::BinaryOp::Multiply => left * right,
        crate::ops::BinaryOp::Divide => left / right,
        crate::ops::BinaryOp::Remainder => left % right,
        crate::ops::BinaryOp::Exponentiate => left.powf(right),
        _ => 0.0,
    }
}

fn unary_plus(value: &Value) -> Result<Value, VmError> {
    if bigint_value(value).is_some() {
        return Err(type_error("Cannot convert BigInt value to number"));
    }
    numeric_unary(value, |n| n)
}

fn has_string_operand(left: &Value, right: &Value) -> bool {
    matches!(left, Value::String(_)) || matches!(right, Value::String(_))
}

fn has_bigint_operand(left: &Value, right: &Value) -> bool {
    bigint_value(left).is_some() || bigint_value(right).is_some()
}

fn is_bigint_arithmetic(operator: crate::ops::BinaryOp) -> bool {
    matches!(
        operator,
        crate::ops::BinaryOp::Add
            | crate::ops::BinaryOp::Subtract
            | crate::ops::BinaryOp::Multiply
            | crate::ops::BinaryOp::Divide
            | crate::ops::BinaryOp::Remainder
            | crate::ops::BinaryOp::Exponentiate
    )
}

fn bigint_error(error: bigint::Error) -> VmError {
    match error {
        bigint::Error::DivisionByZero => range_error("Division by zero"),
        bigint::Error::NegativeExponent => range_error("Exponent must be positive"),
        bigint::Error::ExponentTooLarge => range_error("Maximum BigInt size exceeded"),
        bigint::Error::InvalidDecimal => type_error("Invalid BigInt value"),
    }
}

fn type_error(message: &str) -> VmError {
    VmError::Thrown(crate::builtins::error(
        Builtin::TypeError,
        &[Value::String(message.to_string())],
    ))
}

fn range_error(message: &str) -> VmError {
    VmError::Thrown(crate::builtins::error(
        Builtin::RangeError,
        &[Value::String(message.to_string())],
    ))
}

fn add_string(value: &Value) -> String {
    bigint_value(value)
        .map(str::to_string)
        .unwrap_or_else(|| to_string(Some(value)))
}

fn bigint_value(value: &Value) -> Option<&str> {
    match value {
        Value::BigInt(value) => Some(value.as_str()),
        Value::Object(properties) => properties.iter().find_map(bigint_slot),
        _ => None,
    }
}

fn bigint_slot((key, value): &(String, Value)) -> Option<&str> {
    if key != "_value" {
        return None;
    }
    match value {
        Value::BigInt(value) => Some(value.as_str()),
        _ => None,
    }
}

fn compare_values(
    left: &Value,
    right: &Value,
    compare: fn(f64, f64) -> bool,
) -> Result<Value, VmError> {
    if let (Value::String(left), Value::String(right)) = (left, right) {
        return Ok(Value::Boolean(compare_strings(left, right, compare)));
    }
    let left = to_number(Some(left));
    let right = to_number(Some(right));
    Ok(Value::Boolean(
        !left.is_nan() && !right.is_nan() && compare(left, right),
    ))
}

fn compare_strings(left: &str, right: &str, compare: fn(f64, f64) -> bool) -> bool {
    let ordering = left.cmp(right);
    match ordering {
        std::cmp::Ordering::Less => compare(0.0, 1.0),
        std::cmp::Ordering::Equal => compare(0.0, 0.0),
        std::cmp::Ordering::Greater => compare(1.0, 0.0),
    }
}
