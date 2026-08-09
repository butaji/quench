//! VM op execution helpers (arithmetic, unary, binary, call dispatch).
use crate::intl::tolocale::value::{
    is_truthy, loose_equal, strict_equal, to_int32, to_number, to_string, type_of,
};
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
        UnaryOp::Plus => numeric_unary(value, |number| number)?,
        UnaryOp::Minus => numeric_unary(value, |number| -number)?,
        UnaryOp::Not => Value::Boolean(!is_truthy(&value)),
        UnaryOp::Void => Value::Undefined,
        UnaryOp::Typeof => Value::String(type_of(&value).to_string()),
        UnaryOp::ToString => Value::String(to_string(Some(&value))),
        UnaryOp::Delete => Value::Boolean(true),
    };
    write_value(registers, dst, result);
    Ok(())
}

fn numeric_unary(value: Value, transform: fn(f64) -> f64) -> Result<Value, VmError> {
    Ok(Value::Number(transform(to_number(Some(&value)))))
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
        BinaryOp::Instanceof => Value::Boolean(matches!(
            (left, right),
            (Value::Object(_), Value::Function(_))
        )),
    })
}

fn arithmetic_value(left: &Value, right: &Value, operator: crate::ops::BinaryOp) -> Value {
    if operator == crate::ops::BinaryOp::Add
        && (matches!(left, Value::String(_)) || matches!(right, Value::String(_)))
    {
        return Value::String(format!(
            "{}{}",
            to_string(Some(left)),
            to_string(Some(right))
        ));
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
