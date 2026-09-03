//! VM op execution helpers (arithmetic, unary, binary, call dispatch).
use crate::bigint;
use crate::intl::tolocale::value::{is_truthy, strict_equal, to_int32, type_of};
use crate::ops::Builtin;
use crate::value::Value;

use super::{read_register_unchecked, write_value, VmError};

#[inline(always)]
pub(crate) fn numeric_to_int32(value: f64) -> i32 {
    to_int32(value)
}

#[inline]
pub(crate) fn execute_unary(
    registers: &mut crate::register_file::RegisterFile,
    dst: u16,
    operator: crate::ops::UnaryOp,
    src: u16,
) -> Result<(), VmError> {
    use crate::ops::UnaryOp;
    let value = read_register_unchecked(registers, src);
    let result = match operator {
        UnaryOp::Plus => match value {
            Value::Number(n) => Value::Number(n),
            _ => unary_plus(&value)?,
        },
        UnaryOp::Minus => match value {
            Value::Number(n) => Value::Number(-n),
            _ => unary_minus(&value)?,
        },
        UnaryOp::Not => Value::Boolean(!is_truthy(&value)),
        UnaryOp::BitwiseNot => match value {
            Value::Number(n) => Value::Number(f64::from(!to_int32(n))),
            _ => bitwise_not(&value)?,
        },
        UnaryOp::Void => Value::Undefined,
        UnaryOp::Typeof => Value::String(type_of(&value).to_string()),
        UnaryOp::ToString => to_string_value(&value)?,
        UnaryOp::ToNumeric => to_numeric(&value)?,
        UnaryOp::Delete => Value::Boolean(true),
        UnaryOp::IsNullish => Value::Boolean(matches!(value, Value::Null | Value::Undefined)),
    };
    write_value(registers, dst, result);
    Ok(())
}

/// `ToString` is the identity on string primitives, preserving lone
/// surrogates carried as raw UTF-16 units.
fn to_string_value(value: &Value) -> Result<Value, VmError> {
    if matches!(value, Value::String(_) | Value::StringUnits(_)) {
        return Ok(value.clone());
    }
    Ok(Value::String(crate::conversion::to_string(value)?))
}

fn numeric_unary(value: &Value, transform: fn(f64) -> f64) -> Result<Value, VmError> {
    Ok(Value::Number(transform(crate::conversion::to_number(
        value,
    )?)))
}
#[inline]
pub(crate) fn execute_binary(
    registers: &mut crate::register_file::RegisterFile,
    dst: u16,
    operator: crate::ops::BinaryOp,
    lhs: u16,
    rhs: u16,
) -> Result<(), VmError> {
    if matches!(
        operator,
        crate::ops::BinaryOp::Equal | crate::ops::BinaryOp::NotEqual
    ) {
        if let Some(equal) = registers.abstract_equal_words(usize::from(lhs), usize::from(rhs)) {
            crate::execution_trace::event(crate::execution_trace::Event::EqualityWordHit);
            registers.write_boolean(
                usize::from(dst),
                equal == matches!(operator, crate::ops::BinaryOp::Equal),
            );
            return Ok(());
        }
        crate::execution_trace::event(crate::execution_trace::Event::EqualityWordMiss);
    }
    if let Some((left, right)) = registers.read_number_pair(usize::from(lhs), usize::from(rhs)) {
        use crate::ops::BinaryOp;
        let result = match operator {
            BinaryOp::Add => Some(left + right),
            BinaryOp::Subtract => Some(left - right),
            BinaryOp::Multiply => Some(left * right),
            BinaryOp::Divide => Some(left / right),
            BinaryOp::Remainder => Some(left % right),
            BinaryOp::Exponentiate => Some(exponentiate(left, right)),
            _ => None,
        };
        if let Some(result) = result {
            registers.write_number(usize::from(dst), result);
            return Ok(());
        }
        if let Some(result) = fast_number_binary(left, right, operator) {
            write_value(registers, dst, result);
            return Ok(());
        }
    }
    let left = read_register_unchecked(registers, lhs);
    let right = read_register_unchecked(registers, rhs);
    write_value(registers, dst, evaluate_binary(&left, &right, operator)?);
    Ok(())
}

/// Apply JavaScript's numeric update semantics to one register.
///
/// The compact `IncI` operation is only a physical spelling of `++`/`--`;
/// coercion, BigInt handling, and exceptions remain owned by the canonical
/// numeric-update helper below.
#[inline]
pub(crate) fn execute_numeric_update(
    registers: &mut crate::register_file::RegisterFile,
    dst: u16,
    src: u16,
    decrement: bool,
) -> Result<(), VmError> {
    let value = read_register_unchecked(registers, src);
    let operator = if decrement {
        crate::ops::BinaryOp::NumericSubtract
    } else {
        crate::ops::BinaryOp::NumericAdd
    };
    write_value(registers, dst, numeric_update_value(&value, operator)?);
    Ok(())
}

#[inline]
pub(crate) fn fast_number_binary(
    left: f64,
    right: f64,
    operator: crate::ops::BinaryOp,
) -> Option<Value> {
    use crate::ops::BinaryOp;
    Some(match operator {
        BinaryOp::Add | BinaryOp::Subtract | BinaryOp::Multiply => {
            if !is_negative_zero(left) && !is_negative_zero(right) {
                if let (Some(left_int), Some(right_int)) = (
                    Value::Number(left).as_small_integer(),
                    Value::Number(right).as_small_integer(),
                ) {
                    let checked = match operator {
                        BinaryOp::Add => Value::checked_small_integer_add(left_int, right_int),
                        BinaryOp::Subtract => {
                            Value::checked_small_integer_subtract(left_int, right_int)
                        }
                        BinaryOp::Multiply => {
                            Value::checked_small_integer_multiply(left_int, right_int)
                        }
                        _ => unreachable!(),
                    };
                    if let Some(value) = checked {
                        return Some(value);
                    }
                }
            }
            Value::Number(match operator {
                BinaryOp::Add => left + right,
                BinaryOp::Subtract => left - right,
                BinaryOp::Multiply => left * right,
                _ => unreachable!(),
            })
        }
        BinaryOp::Divide => Value::Number(left / right),
        BinaryOp::Remainder => Value::Number(left % right),
        BinaryOp::Exponentiate => Value::Number(exponentiate(left, right)),
        BinaryOp::LessThan => Value::Boolean(left < right),
        BinaryOp::LessEqual => Value::Boolean(left <= right),
        BinaryOp::GreaterThan => Value::Boolean(left > right),
        BinaryOp::GreaterEqual => Value::Boolean(left >= right),
        BinaryOp::BitwiseOr
        | BinaryOp::BitwiseXor
        | BinaryOp::BitwiseAnd
        | BinaryOp::ShiftLeft
        | BinaryOp::ShiftRight => Value::Number(f64::from(number_bitwise(
            to_int32(left),
            to_int32(right),
            operator,
        ))),
        BinaryOp::ShiftRightZeroFill => Value::Number(f64::from(
            (to_int32(left) as u32) >> shift_count(to_int32(right)),
        )),
        _ => return None,
    })
}

#[inline]
fn is_negative_zero(value: f64) -> bool {
    value == 0.0 && value.is_sign_negative()
}

#[cfg(test)]
mod immediate_integer_tests {
    use super::fast_number_binary;
    use crate::{ops::BinaryOp, value::Value};

    #[test]
    fn add_stays_exact_at_i32_boundaries() {
        assert_eq!(
            fast_number_binary(f64::from(i32::MAX), -1.0, BinaryOp::Add)
                .and_then(|value| value.as_small_integer()),
            Some(i32::MAX - 1)
        );
        let overflow = fast_number_binary(f64::from(i32::MAX), 1.0, BinaryOp::Add).unwrap();
        assert_eq!(overflow, Value::Number(f64::from(i32::MAX) + 1.0));
        assert_eq!(overflow.as_small_integer(), None);
    }

    #[test]
    fn checked_integer_arithmetic_covers_subtract_and_multiply_boundaries() {
        let cases = [
            (BinaryOp::Subtract, i32::MIN, 1, f64::from(i32::MIN) - 1.0),
            (BinaryOp::Multiply, 46_341, 46_341, 46_341.0 * 46_341.0),
        ];
        for (operator, left, right, fallback) in cases {
            let result = fast_number_binary(f64::from(left), f64::from(right), operator)
                .expect("numeric operation");
            assert_eq!(result, Value::Number(fallback));
            assert!(result.as_small_integer().is_none());
        }
        assert_eq!(
            fast_number_binary(7.0, 6.0, BinaryOp::Multiply)
                .and_then(|value| value.as_small_integer()),
            Some(42)
        );
    }
    #[test]
    fn add_preserves_negative_zero_and_fractional_numbers() {
        assert_eq!(
            fast_number_binary(-0.0, 0.0, BinaryOp::Add)
                .unwrap()
                .number_bits(),
            Some(0)
        );
        assert_eq!(
            fast_number_binary(0.5, 0.25, BinaryOp::Add),
            Some(Value::Number(0.75))
        );
        assert_eq!(
            fast_number_binary(-0.0, -0.0, BinaryOp::Add)
                .unwrap()
                .number_bits(),
            Some((-0.0_f64).to_bits())
        );
    }
}
#[inline]
pub(crate) fn evaluate_binary(
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
        BinaryOp::NumericAdd | BinaryOp::NumericSubtract => numeric_update_value(left, operator)?,
        BinaryOp::Equal => Value::Boolean(crate::equality::abstract_equal(left, right)?),
        BinaryOp::NotEqual => Value::Boolean(!crate::equality::abstract_equal(left, right)?),
        BinaryOp::StrictEqual => Value::Boolean(strict_equal(left, right)),
        BinaryOp::StrictNotEqual => Value::Boolean(!strict_equal(left, right)),
        BinaryOp::LessThan => compare_values(left, right, |a, b| a < b)?,
        BinaryOp::LessEqual => compare_values(left, right, |a, b| a <= b)?,
        BinaryOp::GreaterThan => compare_values(left, right, |a, b| a > b)?,
        BinaryOp::GreaterEqual => compare_values(left, right, |a, b| a >= b)?,
        BinaryOp::BitwiseOr
        | BinaryOp::BitwiseXor
        | BinaryOp::BitwiseAnd
        | BinaryOp::ShiftLeft
        | BinaryOp::ShiftRight => bitwise_value(left, right, operator)?,
        BinaryOp::ShiftRightZeroFill => Value::Number(shift_right_unsigned(left, right)?),
        BinaryOp::Instanceof => Value::Boolean(instanceof(left, right)?),
    })
}
include!("vm_instanceof.rs");

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
        BinaryOp::Add => Value::BigInt(bigint::add(&left_s, &right_s).map_err(bigint_error)?),
        BinaryOp::Subtract => {
            Value::BigInt(bigint::subtract(&left_s, &right_s).map_err(bigint_error)?)
        }
        BinaryOp::Multiply => {
            Value::BigInt(bigint::multiply(&left_s, &right_s).map_err(bigint_error)?)
        }
        BinaryOp::Divide => Value::BigInt(bigint::divide(&left_s, &right_s).map_err(bigint_error)?),
        BinaryOp::Remainder => {
            Value::BigInt(bigint::remainder(&left_s, &right_s).map_err(bigint_error)?)
        }
        BinaryOp::Exponentiate => {
            Value::BigInt(bigint::exponentiate(&left_s, &right_s).map_err(bigint_error)?)
        }
        _ => Value::Undefined,
    })
}

#[inline]
fn arithmetic_value(
    left: &Value,
    right: &Value,
    operator: crate::ops::BinaryOp,
) -> Result<Value, VmError> {
    if operator != crate::ops::BinaryOp::Add {
        return numeric_binary_value(left, right, operator);
    }
    // Fast path: ordinary number addition (no string concat, no ToPrimitive).
    if let (Value::Number(l), Value::Number(r)) = (left, right) {
        return Ok(Value::Number(numeric_binary(*l, *r, operator)));
    }
    let left = crate::conversion::to_primitive(left, "default")?;
    let right = crate::conversion::to_primitive(right, "default")?;
    if crate::conversion::is_symbol(&left) || crate::conversion::is_symbol(&right) {
        return Err(crate::value::error::throw_type_error(
            "Cannot convert Symbol value",
        ));
    }
    if let (Value::String(left), Value::String(right)) = (&left, &right) {
        let mut value = String::with_capacity(left.len() + right.len());
        value.push_str(left);
        value.push_str(right);
        return Ok(Value::String(value));
    }
    // `String.fromCodePoint` preserves lone surrogates in `StringUnits`.
    // Reuse that uniquely-owned flat buffer for `+=` so generated Unicode
    // inputs grow amortized-linearly without introducing rope nodes.
    if let Value::StringUnits(mut left_units) = left {
        let right_units = add_units(&right)?;
        if let Some(left_data) = std::rc::Rc::get_mut(&mut left_units) {
            left_data.append_units(&right_units);
            return Ok(Value::StringUnits(left_units));
        }
        let mut units = left_units.to_vec();
        units.extend(right_units);
        return Ok(crate::strings::from_units(units));
    }
    if is_string_like(&left) || is_string_like(&right) {
        let mut units = add_units(&left)?;
        units.extend(add_units(&right)?);
        return Ok(crate::strings::from_units(units));
    }
    if has_bigint_operand(&left, &right) {
        return bigint_binary(&left, &right, operator);
    }
    let left = crate::conversion::primitive_to_number(&left)?;
    let right = crate::conversion::primitive_to_number(&right)?;
    Ok(Value::Number(numeric_binary(left, right, operator)))
}

#[inline]
fn numeric_binary_value(
    left: &Value,
    right: &Value,
    operator: crate::ops::BinaryOp,
) -> Result<Value, VmError> {
    if let (Value::Number(l), Value::Number(r)) = (left, right) {
        return Ok(Value::Number(numeric_binary(*l, *r, operator)));
    }
    let left = to_numeric(left)?;
    let right = to_numeric(right)?;
    if has_bigint_operand(&left, &right) {
        return bigint_binary(&left, &right, operator);
    }
    let left = crate::conversion::to_number(&left)?;
    let right = crate::conversion::to_number(&right)?;
    Ok(Value::Number(numeric_binary(left, right, operator)))
}

/// Implement `++`/`--` semantics: ToNumeric the operand and adjust it by one
/// in its own type, never via string concatenation.
#[inline]
fn numeric_update_value(left: &Value, operator: crate::ops::BinaryOp) -> Result<Value, VmError> {
    let decrement = operator == crate::ops::BinaryOp::NumericSubtract;
    if let Value::Number(n) = left {
        return Ok(Value::Number(if decrement { *n - 1.0 } else { *n + 1.0 }));
    }
    if let Value::BindingCell(cell) = left {
        let value = cell.borrow();
        if let Value::Number(n) = &*value {
            return Ok(Value::Number(if decrement { *n - 1.0 } else { *n + 1.0 }));
        }
    }
    let value = to_numeric(left)?;
    if let Some(big) = bigint_value(&value) {
        let result = if decrement {
            crate::bigint::subtract(&big, "1")
        } else {
            crate::bigint::add(&big, "1")
        };
        return Ok(Value::BigInt(result.map_err(bigint_error)?));
    }
    let n = crate::conversion::to_number(&value)?;
    Ok(Value::Number(if decrement { n - 1.0 } else { n + 1.0 }))
}

/// ECMA-262 `ToNumeric`: coerce to a Number or BigInt, rejecting symbols.
fn to_numeric(value: &Value) -> Result<Value, VmError> {
    let value = crate::conversion::to_primitive(value, "number")?;
    if crate::conversion::is_symbol(&value) {
        return Err(crate::value::error::throw_type_error(
            "Cannot convert Symbol value",
        ));
    }
    if bigint_value(&value).is_some() {
        return Ok(value);
    }
    Ok(Value::Number(crate::conversion::to_number(&value)?))
}

#[inline]
pub(crate) fn evaluate_to_numeric(value: &Value) -> Result<Value, VmError> {
    to_numeric(value)
}

#[inline]
fn bitwise_value(
    left: &Value,
    right: &Value,
    operator: crate::ops::BinaryOp,
) -> Result<Value, VmError> {
    if let (Value::Number(l), Value::Number(r)) = (left, right) {
        let result = number_bitwise(to_int32(*l), to_int32(*r), operator);
        return Ok(Value::Number(f64::from(result)));
    }
    let left = crate::conversion::to_primitive(left, "number")?;
    let left_number = bigint_value(&left)
        .is_none()
        .then(|| crate::conversion::primitive_to_number(&left).map(to_int32));
    let left_number = match left_number {
        Some(value) => Some(value?),
        None => None,
    };
    let right = crate::conversion::to_primitive(right, "number")?;
    if has_bigint_operand(&left, &right) {
        return bigint_bitwise(&left, &right, operator);
    }
    let right = to_int32(crate::conversion::primitive_to_number(&right)?);
    let left = left_number.ok_or_else(|| type_error("Cannot mix BigInt and other types"))?;
    Ok(Value::Number(f64::from(number_bitwise(
        left, right, operator,
    ))))
}

fn bigint_bitwise(
    left: &Value,
    right: &Value,
    operator: crate::ops::BinaryOp,
) -> Result<Value, VmError> {
    use crate::ops::BinaryOp;
    let (Some(left), Some(right)) = (bigint_value(left), bigint_value(right)) else {
        return Err(type_error("Cannot mix BigInt and other types"));
    };
    let result = match operator {
        BinaryOp::BitwiseAnd => bigint::bitwise_and(&left, &right),
        BinaryOp::BitwiseOr => bigint::bitwise_or(&left, &right),
        BinaryOp::BitwiseXor => bigint::bitwise_xor(&left, &right),
        BinaryOp::ShiftLeft => bigint::shift_left(&left, &right),
        BinaryOp::ShiftRight => bigint::shift_right(&left, &right),
        _ => return Err(type_error("Invalid BigInt operator")),
    };
    Ok(Value::BigInt(result.map_err(bigint_error)?))
}

fn number_bitwise(left: i32, right: i32, operator: crate::ops::BinaryOp) -> i32 {
    match operator {
        crate::ops::BinaryOp::BitwiseAnd => left & right,
        crate::ops::BinaryOp::BitwiseOr => left | right,
        crate::ops::BinaryOp::BitwiseXor => left ^ right,
        crate::ops::BinaryOp::ShiftLeft => left.wrapping_shl(shift_count(right)),
        crate::ops::BinaryOp::ShiftRight => left.wrapping_shr(shift_count(right)),
        _ => 0,
    }
}

/// Shift amount: the low 5 bits of the right operand, per ECMAScript.
fn shift_count(right: i32) -> u32 {
    (right as u32) & 31
}

/// ECMAScript unsigned right shift: ToUint32(left) >> (count & 31), as a number.
#[inline]
fn shift_right_unsigned(left: &Value, right: &Value) -> Result<f64, VmError> {
    if let (Value::Number(l), Value::Number(r)) = (left, right) {
        let left = to_int32(*l) as u32;
        let right = to_int32(*r);
        return Ok(f64::from(left >> shift_count(right)));
    }
    let left = crate::conversion::to_primitive(left, "number")?;
    let left_number = bigint_value(&left)
        .is_none()
        .then(|| crate::conversion::primitive_to_number(&left).map(to_int32));
    let left_number = left_number.transpose()?;
    let right = crate::conversion::to_primitive(right, "number")?;
    if has_bigint_operand(&left, &right) {
        return Err(type_error("BigInt has no unsigned right shift"));
    }
    let left = left_number.ok_or_else(|| type_error("BigInt has no unsigned right shift"))? as u32;
    let right = to_int32(crate::conversion::primitive_to_number(&right)?);
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

#[inline]
fn unary_plus(value: &Value) -> Result<Value, VmError> {
    if let Value::Number(n) = value {
        return Ok(Value::Number(*n));
    }
    let value = crate::conversion::to_primitive(value, "number")?;
    if bigint_value(&value).is_some() {
        return Err(type_error("Cannot convert BigInt value to number"));
    }
    numeric_unary(&value, |n| n)
}

#[inline]
fn unary_minus(value: &Value) -> Result<Value, VmError> {
    if let Value::Number(n) = value {
        return Ok(Value::Number(-*n));
    }
    let value = crate::conversion::to_primitive(value, "number")?;
    if let Some(value) = bigint_value(&value) {
        return Ok(Value::BigInt(bigint::negate(&value).map_err(bigint_error)?));
    }
    Ok(Value::Number(-crate::conversion::primitive_to_number(
        &value,
    )?))
}

#[inline]
fn bitwise_not(value: &Value) -> Result<Value, VmError> {
    if let Value::Number(n) = value {
        return Ok(Value::Number(f64::from(!to_int32(*n))));
    }
    let value = crate::conversion::to_primitive(value, "number")?;
    if let Some(value) = bigint_value(&value) {
        let result = bigint::subtract("-1", &value).map_err(bigint_error)?;
        return Ok(Value::BigInt(result));
    }
    let number = crate::conversion::primitive_to_number(&value)?;
    Ok(Value::Number(f64::from(!to_int32(number))))
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

fn is_string_like(value: &Value) -> bool {
    matches!(value, Value::String(_) | Value::StringUnits(_))
}

fn add_units(value: &Value) -> Result<Vec<u16>, VmError> {
    if let Value::StringUnits(units) = value {
        return Ok((**units).to_vec());
    }
    Ok(add_string(value)?.encode_utf16().collect())
}

fn add_string(value: &Value) -> Result<String, VmError> {
    if crate::conversion::is_symbol(value) {
        return Err(type_error("Cannot convert Symbol value to string"));
    }
    Ok(bigint_value(value)
        .map(|value| value.to_string())
        .unwrap_or_else(|| crate::intl::tolocale::value::to_string(Some(value))))
}

fn bigint_value(value: &Value) -> Option<String> {
    match value {
        Value::BigInt(value) => Some(value.clone()),
        Value::Object(properties) => properties.iter().find_map(bigint_slot),
        _ => None,
    }
}

fn bigint_slot((key, value): (&crate::value::PropertyName, Value)) -> Option<String> {
    if key != "_value" {
        return None;
    }
    match value {
        Value::BigInt(value) => Some(value),
        _ => None,
    }
}

include!("vm_compare.rs");

#[cfg(test)]
mod fast_path_tests {
    use super::{evaluate_binary, fast_number_binary};
    use crate::{ops::BinaryOp, value::Value};

    #[test]
    fn numeric_add_uses_specialized_result() {
        assert_eq!(
            fast_number_binary(2.0, 3.0, BinaryOp::Add),
            Some(Value::Number(5.0))
        );
    }

    #[test]
    fn non_numeric_add_falls_back_to_generic_path() {
        let left = Value::String("a".to_string());
        let right = Value::Number(1.0);
        assert_eq!(
            evaluate_binary(&left, &right, BinaryOp::Add).unwrap(),
            Value::String("a1".to_string())
        );
    }

    #[test]
    fn unsupported_operator_falls_back_to_generic_path() {
        let left = Value::Number(2.0);
        let right = Value::Number(2.0);
        assert_eq!(fast_number_binary(2.0, 2.0, BinaryOp::Equal), None);
        assert_eq!(
            evaluate_binary(&left, &right, BinaryOp::Equal).unwrap(),
            Value::Boolean(true)
        );
    }

    #[test]
    fn generic_path_reports_coercion_errors() {
        let left = Value::BigInt("1".to_string());
        let right = Value::Number(1.0);
        assert!(evaluate_binary(&left, &right, BinaryOp::Add).is_err());
    }

    #[test]
    fn specialized_numeric_results_match_generic_results() {
        let left = Value::Number(8.0);
        let right = Value::Number(3.0);
        for operator in [
            BinaryOp::Add,
            BinaryOp::Subtract,
            BinaryOp::Multiply,
            BinaryOp::Divide,
            BinaryOp::Remainder,
            BinaryOp::Exponentiate,
            BinaryOp::LessThan,
            BinaryOp::LessEqual,
            BinaryOp::GreaterThan,
            BinaryOp::GreaterEqual,
        ] {
            assert_eq!(
                fast_number_binary(8.0, 3.0, operator),
                Some(evaluate_binary(&left, &right, operator).unwrap()),
                "operator {operator:?}"
            );
        }
    }
}
