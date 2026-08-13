use std::rc::Rc;

use crate::{execute::VmError, value::Value};

pub(crate) fn abstract_equal(left: &Value, right: &Value) -> Result<bool, VmError> {
    if same_ecmascript_type(left, right) {
        return Ok(strict_equal(left, right));
    }
    if is_nullish_pair(left, right) {
        return Ok(true);
    }
    if let Some(values) = boolean_pair(left, right) {
        return abstract_equal(&values.0, &values.1);
    }
    if let Some(result) = number_string_equal(left, right) {
        return Ok(result);
    }
    if let Some(result) = bigint_string_equal(left, right) {
        return Ok(result);
    }
    if let Some(result) = bigint_number_equal(left, right) {
        return Ok(result);
    }
    if is_coercible_primitive(left) && is_equality_object(right) {
        return abstract_equal(left, &crate::conversion::to_primitive(right, "default")?);
    }
    if is_equality_object(left) && is_coercible_primitive(right) {
        return abstract_equal(&crate::conversion::to_primitive(left, "default")?, right);
    }
    Ok(false)
}

fn same_ecmascript_type(left: &Value, right: &Value) -> bool {
    if crate::conversion::is_symbol(left) || crate::conversion::is_symbol(right) {
        return crate::conversion::is_symbol(left) && crate::conversion::is_symbol(right);
    }
    std::mem::discriminant(left) == std::mem::discriminant(right)
}

fn is_nullish_pair(left: &Value, right: &Value) -> bool {
    matches!(
        (left, right),
        (Value::Null, Value::Undefined) | (Value::Undefined, Value::Null)
    )
}

fn boolean_pair(left: &Value, right: &Value) -> Option<(Value, Value)> {
    match (left, right) {
        (Value::Boolean(value), right) => Some((Value::Number(f64::from(*value)), right.clone())),
        (left, Value::Boolean(value)) => Some((left.clone(), Value::Number(f64::from(*value)))),
        _ => None,
    }
}

fn number_string_equal(left: &Value, right: &Value) -> Option<bool> {
    match (left, right) {
        (Value::Number(number), Value::String(string))
        | (Value::String(string), Value::Number(number)) => Some(
            *number
                == crate::intl::tolocale::value::to_number(Some(&Value::String(string.clone()))),
        ),
        _ => None,
    }
}

fn bigint_string_equal(left: &Value, right: &Value) -> Option<bool> {
    let (bigint, string) = match (left, right) {
        (Value::BigInt(bigint), Value::String(string))
        | (Value::String(string), Value::BigInt(bigint)) => (bigint, string),
        _ => return None,
    };
    Some(crate::bigint::parse_string(string).is_some_and(|value| value.to_string() == *bigint))
}

fn bigint_number_equal(left: &Value, right: &Value) -> Option<bool> {
    let (bigint, number) = match (left, right) {
        (Value::BigInt(bigint), Value::Number(number))
        | (Value::Number(number), Value::BigInt(bigint)) => (bigint, *number),
        _ => return None,
    };
    Some(number_bigint(number).is_some_and(|number| number.to_string() == *bigint))
}

fn number_bigint(value: f64) -> Option<num_bigint::BigInt> {
    if !value.is_finite() || value.fract() != 0.0 {
        return None;
    }
    if value == 0.0 {
        return Some(0.into());
    }
    let bits = value.abs().to_bits();
    let exponent = i32::from(u16::try_from((bits >> 52) & 0x7ff).ok()?) - 1023;
    let significand = (bits & ((1_u64 << 52) - 1)) | (1_u64 << 52);
    let mut integer = if exponent >= 52 {
        num_bigint::BigInt::from(significand) << usize::try_from(exponent - 52).ok()?
    } else {
        num_bigint::BigInt::from(significand >> u32::try_from(52 - exponent).ok()?)
    };
    if value.is_sign_negative() {
        integer = -integer;
    }
    Some(integer)
}

fn is_primitive(value: &Value) -> bool {
    !is_equality_object(value)
}

fn is_coercible_primitive(value: &Value) -> bool {
    is_primitive(value) && !matches!(value, Value::Null | Value::Undefined)
}

fn is_equality_object(value: &Value) -> bool {
    crate::value::is_object(value) && !crate::conversion::is_symbol(value)
}

pub(crate) fn strict_equal(left: &Value, right: &Value) -> bool {
    match (left, right) {
        (Value::Array(left), Value::Array(right)) => Rc::ptr_eq(left, right),
        (Value::Object(left), Value::Object(right)) => Rc::ptr_eq(left, right),
        (Value::ObjectAlias(left), Value::Object(right))
        | (Value::Object(right), Value::ObjectAlias(left)) => left
            .0
            .borrow()
            .upgrade()
            .is_some_and(|left| Rc::ptr_eq(&left, right)),
        (Value::ArrayBuffer(left), Value::ArrayBuffer(right)) => Rc::ptr_eq(left, right),
        (Value::DataView(left), Value::DataView(right)) => Rc::ptr_eq(left, right),
        (Value::Float32Array(left), Value::Float32Array(right)) => Rc::ptr_eq(left, right),
        (Value::Float64Array(left), Value::Float64Array(right)) => Rc::ptr_eq(left, right),
        (Value::Int16Array(left), Value::Int16Array(right)) => Rc::ptr_eq(left, right),
        (Value::Int8Array(left), Value::Int8Array(right)) => Rc::ptr_eq(left, right),
        (Value::Int32Array(left), Value::Int32Array(right)) => Rc::ptr_eq(left, right),
        (Value::Uint16Array(left), Value::Uint16Array(right)) => Rc::ptr_eq(left, right),
        (Value::Uint32Array(left), Value::Uint32Array(right)) => Rc::ptr_eq(left, right),
        (Value::Uint8Array(left), Value::Uint8Array(right)) => Rc::ptr_eq(left, right),
        (Value::Uint8ClampedArray(left), Value::Uint8ClampedArray(right)) => {
            Rc::ptr_eq(left, right)
        }
        (Value::Function(left), Value::Function(right)) => Rc::ptr_eq(left, right),
        (Value::BoundFunction(left), Value::BoundFunction(right)) => Rc::ptr_eq(left, right),
        (Value::Generator(left), Value::Generator(right)) => Rc::ptr_eq(left, right),
        (Value::Number(left), Value::Number(right)) => left == right,
        (Value::Boolean(left), Value::Boolean(right)) => left == right,
        (Value::String(left), Value::String(right)) => left == right,
        (Value::StringUnits(_), Value::String(_))
        | (Value::String(_), Value::StringUnits(_))
        | (Value::StringUnits(_), Value::StringUnits(_)) => {
            crate::strings::units_equal(left, right)
        }
        (Value::BigInt(left), Value::BigInt(right)) => left == right,
        (Value::Builtin(left), Value::Builtin(right)) => left == right,
        (Value::Null, Value::Null) | (Value::Undefined, Value::Undefined) => true,
        _ => false,
    }
}
