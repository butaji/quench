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
        if let Some(value) = digits.filter(|value| !matches!(value, Value::Undefined)) {
            if builtin == Builtin::NumberToFixed {
                let _ = to_digits(value)?;
            } else {
                let _ = crate::conversion::to_number(value)?;
            }
        }
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
        Some(digits) => exponential_scientific(number, digits),
        None => default_exponential(number),
    })
}

fn scientific(number: f64, digits: usize) -> String {
    formatted_scientific(number, digits, false)
}

fn exponential_scientific(number: f64, digits: usize) -> String {
    formatted_scientific(number, digits, true)
}

fn formatted_scientific(number: f64, digits: usize, half_up: bool) -> String {
    let value = format!("{number:.digits$e}");
    let (coefficient, exponent) = value.split_once('e').unwrap_or((&value, "0"));
    let exponent = exponent.parse::<i32>().unwrap_or_default();
    let rounded = half_up
        .then(|| halfway_coefficient(number, digits, exponent))
        .flatten();
    let (coefficient, exponent) = rounded.unwrap_or_else(|| (coefficient.to_string(), exponent));
    format!("{coefficient}e{exponent:+}")
}

fn halfway_coefficient(number: f64, digits: usize, exponent: i32) -> Option<(String, i32)> {
    let scale = 10_f64.powi(exponent - digits as i32);
    let scaled = number.abs() / scale;
    (scaled.fract() == 0.5).then(|| {
        let mut rounded = scaled.floor() + 1.0;
        let mut exponent = exponent;
        if rounded == 10_f64.powi(digits as i32 + 1) {
            rounded /= 10.0;
            exponent += 1;
        }
        let coefficient = rounded / 10_f64.powi(digits as i32);
        let sign = if number.is_sign_negative() { "-" } else { "" };
        (format!("{sign}{coefficient:.digits$}"), exponent)
    })
}

fn default_exponential(number: f64) -> String {
    let value = scientific(number, 15);
    let (coefficient, exponent) = value.split_once('e').unwrap_or((&value, "0"));
    let coefficient = coefficient.trim_end_matches('0').trim_end_matches('.');
    format!("{coefficient}e{exponent}")
}
