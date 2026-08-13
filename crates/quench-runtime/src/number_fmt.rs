//! `Number.prototype` formatting helpers (`toFixed`, `toPrecision`, `toExponential`).

use crate::{execute::VmError, ops::Builtin, value::Value};

/// Dispatch a `Number.prototype` formatting method.
pub(crate) fn number_format(
    value: Option<&Value>,
    digits: Option<&Value>,
    builtin: Builtin,
) -> Result<Value, VmError> {
    let Value::Number(number) = crate::vm::number_value_of(value)? else {
        return Err(crate::value::error::throw_type_error(
            "Number method called on incompatible receiver",
        ));
    };
    let number = if number == 0.0 { 0.0 } else { number };
    if !number.is_finite() {
        return Ok(Value::String(crate::conversion::number_to_string(number)));
    }
    let digits = digits
        .filter(|value| !matches!(value, Value::Undefined))
        .map(to_digits)
        .transpose()?;
    let text = match builtin {
        Builtin::NumberToFixed => fixed(number, digits.unwrap_or(0))?,
        Builtin::NumberToPrecision => precision(number, digits)?,
        Builtin::NumberToExponential => exponential(number, digits)?,
        _ => return Ok(Value::Undefined),
    };
    Ok(Value::String(text))
}

fn to_digits(value: &Value) -> Result<usize, VmError> {
    let value = crate::conversion::to_number(value)?;
    if value.is_nan() {
        return Ok(0);
    }
    if !value.is_finite() || !(0.0..=100.0).contains(&value.trunc()) {
        return Err(crate::value::error::throw_range_error("Invalid precision"));
    }
    Ok(value.trunc() as usize)
}

fn fixed(number: f64, digits: usize) -> Result<String, VmError> {
    if !number.is_finite() || number.abs() >= 1e21 {
        return Ok(crate::conversion::number_to_string(number));
    }
    Ok(format!("{number:.digits$}"))
}

fn precision(number: f64, digits: Option<usize>) -> Result<String, VmError> {
    if !number.is_finite() || digits.is_none() {
        return Ok(crate::conversion::number_to_string(number));
    }
    let digits = digits.unwrap_or_default();
    if digits == 0 {
        return Err(crate::value::error::throw_range_error("Invalid precision"));
    }
    let magnitude = if number == 0.0 {
        0
    } else {
        number.abs().log10().floor() as i32
    };
    if magnitude >= digits as i32 || magnitude < -6 {
        Ok(scientific(number, digits - 1))
    } else {
        Ok(format!(
            "{number:.precision$}",
            precision = (digits as i32 - magnitude - 1) as usize
        ))
    }
}

fn exponential(number: f64, digits: Option<usize>) -> Result<String, VmError> {
    if !number.is_finite() {
        return Ok(crate::conversion::number_to_string(number));
    }
    Ok(match digits {
        Some(digits) => scientific(number, digits),
        None => default_exponential(number),
    })
}

fn scientific(number: f64, digits: usize) -> String {
    let value = format!("{number:.digits$e}");
    let (coefficient, exponent) = value.split_once('e').unwrap_or((&value, "0"));
    let exponent = exponent.parse::<i32>().unwrap_or_default();
    format!("{coefficient}e{exponent:+}")
}

fn default_exponential(number: f64) -> String {
    let value = scientific(number, 15);
    let (coefficient, exponent) = value.split_once('e').unwrap_or((&value, "0"));
    let coefficient = coefficient.trim_end_matches('0').trim_end_matches('.');
    format!("{coefficient}e{exponent}")
}
