//! Date helper functions.

use crate::value::Value;

/// Convert Value to f64, returning NaN for non-numeric.
pub fn to_number(v: &Value) -> f64 {
    match v {
        Value::Number(n) => *n,
        Value::String(s) => s.parse().unwrap_or(f64::NAN),
        Value::Boolean(b) => {
            if *b {
                1.0
            } else {
                0.0
            }
        }
        Value::Null => 0.0,
        _ => f64::NAN,
    }
}

/// Convert Value to i32, returning 0 for non-integer.
pub fn to_int32(v: &Value) -> f64 {
    let n = to_number(v);
    if n.is_nan() || n.is_infinite() {
        return 0.0;
    }
    let int = n.trunc();
    if int.is_nan() || int.is_infinite() {
        return 0.0;
    }
    int as i32 as f64
}

/// Convert Value to String.
pub fn value_to_string(v: &Value) -> String {
    match v {
        Value::String(s) => s.clone(),
        Value::Number(n) => n.to_string(),
        Value::Boolean(b) => b.to_string(),
        Value::Null => "null".to_string(),
        Value::Undefined => "undefined".to_string(),
        _ => String::new(),
    }
}
