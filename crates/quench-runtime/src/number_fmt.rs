//! `Number.prototype` formatting helpers (`toFixed`, `toPrecision`, `toExponential`).

use crate::{execute::VmError, ops::Builtin, value::Value};

/// Dispatch a `Number.prototype` formatting method.
pub(crate) fn number_format(
    value: Option<&Value>,
    digits: Option<&Value>,
    builtin: Builtin,
) -> Result<Value, VmError> {
    let number = to_number(value);
    let digits = to_number(digits) as usize;
    let text = match builtin {
        Builtin::NumberToFixed => fixed(number, digits),
        Builtin::NumberToPrecision => precision(number, digits),
        Builtin::NumberToExponential => exponential(number, digits),
        _ => return Ok(Value::Undefined),
    };
    Ok(Value::String(text))
}

fn to_number(value: Option<&Value>) -> f64 {
    match value {
        None | Some(Value::Undefined) => f64::NAN,
        Some(Value::Null) => 0.0,
        Some(Value::Boolean(value)) => f64::from(*value),
        Some(Value::Number(value)) => *value,
        Some(Value::String(value)) => value.trim().parse().unwrap_or(f64::NAN),
        _ => f64::NAN,
    }
}

fn fixed(number: f64, digits: usize) -> String {
    if digits > 100 {
        return "RangeError".to_string();
    }
    format!("{:.*}", digits, number)
}

fn precision(number: f64, digits: usize) -> String {
    if digits == 0 || digits > 100 {
        return "RangeError".to_string();
    }
    let magnitude = if number == 0.0 {
        0
    } else {
        number.abs().log10().floor() as i32
    };
    if magnitude >= digits as i32 || magnitude < 0 {
        format!("{:.*e}", digits - 1, number)
    } else {
        format!("{:.*}", digits - magnitude as usize - 1, number)
    }
}

fn exponential(number: f64, digits: usize) -> String {
    if digits > 100 {
        return "RangeError".to_string();
    }
    format!("{:.*e}", digits, number)
}
