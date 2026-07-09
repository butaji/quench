//! VM helper functions.

use crate::nanbox::JSValue;

/// Convert JSValue to number for arithmetic operations.
pub fn to_number(v: JSValue) -> f64 {
    if v.is_int32() {
        v.as_int32_unchecked() as f64
    } else if v.is_double() {
        v.as_double_unchecked()
    } else if v.is_undefined() {
        f64::NAN
    } else if v.is_null() {
        0.0
    } else if v.is_true() {
        1.0
    } else if v.is_false() {
        0.0
    } else {
        f64::NAN
    }
}

/// Convert legacy Value to JSValue.
pub fn legacy_to_jsvalue(v: Option<crate::value::Value>) -> JSValue {
    use crate::value::Value;
    match v {
        None | Some(Value::Undefined) => JSValue::undefined(),
        Some(Value::Null) => JSValue::null(),
        Some(Value::Boolean(b)) => JSValue::bool(b),
        Some(Value::Number(n)) => JSValue::double(n),
        Some(Value::String(_s)) => JSValue::double(f64::NAN),
        Some(Value::ObjectId(id)) => JSValue::object(id),
        _ => JSValue::undefined(),
    }
}
