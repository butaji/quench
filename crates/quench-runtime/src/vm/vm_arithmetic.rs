//! VM op execution helpers (arithmetic, unary, binary, call dispatch).
use crate::bigint;
use crate::intl::tolocale::value::{is_truthy, strict_equal, to_int32, to_string, type_of};
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
    Ok(Value::Number(transform(crate::conversion::to_number(
        value,
    )?)))
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
    Ok(match operator {
        BinaryOp::Add
        | BinaryOp::Subtract
        | BinaryOp::Multiply
        | BinaryOp::Divide
        | BinaryOp::Remainder
        | BinaryOp::Exponentiate => arithmetic_value(left, right, operator)?,
        BinaryOp::Equal => Value::Boolean(crate::equality::abstract_equal(left, right)?),
        BinaryOp::NotEqual => Value::Boolean(!crate::equality::abstract_equal(left, right)?),
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
        BinaryOp::ShiftRightZeroFill => Value::Number(shift_right_unsigned(left, right)?),
        BinaryOp::Instanceof => Value::Boolean(instanceof(left, right)),
    })
}

fn instanceof(value: &Value, constructor: &Value) -> bool {
    if matches!(
        (value, constructor),
        (
            Value::BigInt64Array(_),
            Value::Builtin(Builtin::BigInt64Array)
        ) | (
            Value::BigUint64Array(_),
            Value::Builtin(Builtin::BigUint64Array)
        )
    ) {
        return true;
    }
    if matches!(
        (value, constructor),
        (Value::Map(_), Value::Builtin(Builtin::Map))
    ) {
        return true;
    }
    if matches!(
        (value, constructor),
        (Value::Set(_), Value::Builtin(Builtin::Set))
    ) {
        return true;
    }
    let prototype = crate::execute::get_property(constructor, "prototype");
    if crate::value::is_object(&prototype) && prototype_chain_contains(value, &prototype) {
        return true;
    }
    own_constructor(value)
        .is_some_and(|value| crate::builtins::same_value(Some(&value), Some(constructor)))
}

fn prototype_chain_contains(value: &Value, expected: &Value) -> bool {
    let mut current = internal_prototype(value);
    for _ in 0..1_024 {
        let Some(prototype) = current else {
            return false;
        };
        if crate::builtins::same_value(Some(&prototype), Some(expected)) {
            return true;
        }
        current = internal_prototype(&prototype);
    }
    false
}

fn internal_prototype(value: &Value) -> Option<Value> {
    let Value::Object(properties) = value else {
        return None;
    };
    properties
        .iter()
        .rev()
        .find_map(|(name, value)| (name == "\0prototype").then(|| value.clone()))
}

fn own_constructor(value: &Value) -> Option<Value> {
    let Value::Object(properties) = value else {
        return None;
    };
    properties
        .iter()
        .rev()
        .find_map(|(name, value)| (name == "constructor").then(|| value.clone()))
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

fn arithmetic_value(
    left: &Value,
    right: &Value,
    operator: crate::ops::BinaryOp,
) -> Result<Value, VmError> {
    let left = crate::conversion::to_primitive(left, "default")?;
    let right = crate::conversion::to_primitive(right, "default")?;
    if operator == crate::ops::BinaryOp::Add
        && (matches!(left, Value::String(_)) || matches!(right, Value::String(_)))
    {
        return Ok(Value::String(format!(
            "{}{}",
            add_string(&left)?,
            add_string(&right)?
        )));
    }
    if has_bigint_operand(&left, &right) {
        return bigint_binary(&left, &right, operator);
    }
    let left = crate::conversion::primitive_to_number(&left)?;
    let right = crate::conversion::primitive_to_number(&right)?;
    Ok(Value::Number(numeric_binary(left, right, operator)))
}

fn bitwise_numbers(
    left: &Value,
    right: &Value,
    operation: fn(i32, i32) -> i32,
) -> Result<Value, VmError> {
    let left = to_int32(crate::conversion::to_number(left)?);
    let right = to_int32(crate::conversion::to_number(right)?);
    Ok(Value::Number(f64::from(operation(left, right))))
}

/// Shift amount: the low 5 bits of the right operand, per ECMAScript.
fn shift_count(right: i32) -> u32 {
    (right as u32) & 31
}

/// ECMAScript unsigned right shift: ToUint32(left) >> (count & 31), as a number.
fn shift_right_unsigned(left: &Value, right: &Value) -> Result<f64, VmError> {
    let left = to_int32(crate::conversion::to_number(left)?) as u32;
    let right = to_int32(crate::conversion::to_number(right)?);
    Ok(f64::from(left >> shift_count(right)))
}

fn numeric_binary(left: f64, right: f64, operator: crate::ops::BinaryOp) -> f64 {
    match operator {
        crate::ops::BinaryOp::Add => left + right,
        crate::ops::BinaryOp::Subtract => left - right,
        crate::ops::BinaryOp::Multiply => left * right,
        crate::ops::BinaryOp::Divide => left / right,
        crate::ops::BinaryOp::Remainder => left % right,
        crate::ops::BinaryOp::Exponentiate => exponentiate(left, right),
        _ => 0.0,
    }
}

fn exponentiate(base: f64, exponent: f64) -> f64 {
    if exponent.is_nan() {
        return f64::NAN;
    }
    if exponent == 0.0 {
        return 1.0;
    }
    if base.is_nan() || base.abs() == 1.0 && exponent.is_infinite() {
        return f64::NAN;
    }
    if base.is_infinite() {
        return infinite_power(base, exponent);
    }
    if base == 0.0 {
        return zero_power(base, exponent);
    }
    base.powf(exponent)
}

fn infinite_power(base: f64, exponent: f64) -> f64 {
    let magnitude = if exponent.is_sign_positive() {
        f64::INFINITY
    } else {
        0.0
    };
    if base.is_sign_negative() && is_odd_integer(exponent) {
        -magnitude
    } else {
        magnitude
    }
}

fn zero_power(base: f64, exponent: f64) -> f64 {
    let magnitude = if exponent.is_sign_positive() {
        0.0
    } else {
        f64::INFINITY
    };
    if base.is_sign_negative() && is_odd_integer(exponent) {
        -magnitude
    } else {
        magnitude
    }
}

fn is_odd_integer(value: f64) -> bool {
    value.is_finite() && value.fract() == 0.0 && value.abs() % 2.0 == 1.0
}

fn unary_plus(value: &Value) -> Result<Value, VmError> {
    if bigint_value(value).is_some() {
        return Err(type_error("Cannot convert BigInt value to number"));
    }
    numeric_unary(value, |n| n)
}

fn has_bigint_operand(left: &Value, right: &Value) -> bool {
    bigint_value(left).is_some() || bigint_value(right).is_some()
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

fn add_string(value: &Value) -> Result<String, VmError> {
    if crate::conversion::is_symbol(value) {
        return Err(type_error("Cannot convert Symbol value to string"));
    }
    Ok(bigint_value(value)
        .map(str::to_string)
        .unwrap_or_else(|| to_string(Some(value))))
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
    let left = crate::conversion::to_primitive(left, "number")?;
    let right = crate::conversion::to_primitive(right, "number")?;
    if let (Value::String(left), Value::String(right)) = (&left, &right) {
        return Ok(Value::Boolean(compare_strings(left, right, compare)));
    }
    let left = crate::conversion::primitive_to_number(&left)?;
    let right = crate::conversion::primitive_to_number(&right)?;
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
