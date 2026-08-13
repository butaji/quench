//! Completion-aware Date setters.

use crate::{execute::VmError, ops::Builtin, value::Value};

use super::{chrono_utils, extract_time, store_time};

pub fn set_date(receiver: Option<&Value>, arguments: &[Value]) -> Result<Value, VmError> {
    let current = time_value(receiver)?;
    let day = argument(arguments, 0, f64::NAN)?;
    if current.is_nan() {
        return Ok(Value::Number(f64::NAN));
    }
    let result = chrono_utils::local_components(current).map_or(f64::NAN, |parts| {
        local_time(parts, parts.0 as f64, (parts.1 - 1) as f64, day)
    });
    store(receiver, result)
}

pub fn set_utc_date(receiver: Option<&Value>, arguments: &[Value]) -> Result<Value, VmError> {
    let current = time_value(receiver)?;
    let day = argument(arguments, 0, f64::NAN)?;
    if current.is_nan() {
        return Ok(Value::Number(f64::NAN));
    }
    let result = chrono_utils::utc_components(current).map_or(f64::NAN, |parts| {
        utc_time(parts, parts.0 as f64, (parts.1 - 1) as f64, day)
    });
    store(receiver, result)
}

pub fn set_utc_month(receiver: Option<&Value>, arguments: &[Value]) -> Result<Value, VmError> {
    let current = time_value(receiver)?;
    let parts = chrono_utils::utc_components(current);
    let month = argument(arguments, 0, f64::NAN)?;
    let day = argument(arguments, 1, parts.map_or(f64::NAN, |parts| parts.2 as f64))?;
    if current.is_nan() {
        return Ok(Value::Number(f64::NAN));
    }
    let result = parts.map_or(f64::NAN, |parts| {
        utc_time(parts, parts.0 as f64, month, day)
    });
    store(receiver, result)
}

pub fn set_utc_full_year(receiver: Option<&Value>, arguments: &[Value]) -> Result<Value, VmError> {
    let current = time_value(receiver)?;
    let parts = chrono_utils::utc_components(if current.is_nan() { 0.0 } else { current })
        .unwrap_or((1970, 1, 1, 0, 0, 0, 0));
    let year = argument(arguments, 0, f64::NAN)?;
    let month = argument(arguments, 1, (parts.1 - 1) as f64)?;
    let day = argument(arguments, 2, parts.2 as f64)?;
    store(receiver, utc_time(parts, year, month, day))
}

pub fn set_full_year(receiver: Option<&Value>, arguments: &[Value]) -> Result<Value, VmError> {
    let current = time_value(receiver)?;
    let parts = if current.is_nan() {
        chrono_utils::utc_components(0.0)
    } else {
        chrono_utils::local_components(current)
    }
    .unwrap_or((1970, 1, 1, 0, 0, 0, 0));
    let year = argument(arguments, 0, f64::NAN)?;
    let month = argument(arguments, 1, (parts.1 - 1) as f64)?;
    let day = argument(arguments, 2, parts.2 as f64)?;
    let result = local_time(parts, year, month, day);
    store(receiver, result)
}

pub fn set_month(receiver: Option<&Value>, arguments: &[Value]) -> Result<Value, VmError> {
    let current = time_value(receiver)?;
    let parts = chrono_utils::local_components(current);
    let month = argument(arguments, 0, f64::NAN)?;
    let day = argument(arguments, 1, parts.map_or(f64::NAN, |parts| parts.2 as f64))?;
    if current.is_nan() {
        return Ok(Value::Number(f64::NAN));
    }
    let result = parts.map_or(f64::NAN, |parts| {
        local_time(parts, parts.0 as f64, month, day)
    });
    store(receiver, result)
}

pub fn set_time(receiver: Option<&Value>, arguments: &[Value]) -> Result<Value, VmError> {
    time_value(receiver)?;
    store(receiver, argument(arguments, 0, f64::NAN)?)
}

pub fn set_time_components(
    builtin: Builtin,
    receiver: Option<&Value>,
    arguments: &[Value],
) -> Option<Result<Value, VmError>> {
    let (start, utc) = match builtin {
        Builtin::DateSetHours => (0, false),
        Builtin::DateSetMinutes => (1, false),
        Builtin::DateSetSeconds => (2, false),
        Builtin::DateSetMilliseconds => (3, false),
        Builtin::DateSetUTCHours => (0, true),
        Builtin::DateSetUTCMinutes => (1, true),
        Builtin::DateSetUTCSeconds => (2, true),
        Builtin::DateSetUTCMilliseconds => (3, true),
        _ => return None,
    };
    Some(set_components(receiver, arguments, start, utc))
}

fn set_components(
    receiver: Option<&Value>,
    arguments: &[Value],
    start: usize,
    utc: bool,
) -> Result<Value, VmError> {
    let current = time_value(receiver)?;
    let parts = if utc {
        chrono_utils::utc_components(current)
    } else {
        chrono_utils::local_components(current)
    };
    let values = components(arguments, start, parts.map_or([0.0; 4], time_values))?;
    if current.is_nan() {
        return Ok(Value::Number(f64::NAN));
    }
    let result = parts.map_or(f64::NAN, |parts| make_time(parts, values, utc));
    store(receiver, result)
}

fn components(
    arguments: &[Value],
    start: usize,
    mut values: [f64; 4],
) -> Result<[f64; 4], VmError> {
    for index in start..values.len() {
        let value = arguments
            .get(index - start)
            .map(crate::conversion::to_number)
            .transpose()?;
        if index == start {
            values[index] = value.unwrap_or(f64::NAN);
        } else if let Some(value) = value {
            values[index] = value;
        }
    }
    Ok(values)
}

fn time_values(parts: (i32, u32, u32, u32, u32, u32, u32)) -> [f64; 4] {
    [
        parts.3 as f64,
        parts.4 as f64,
        parts.5 as f64,
        parts.6 as f64,
    ]
}

fn make_time(parts: (i32, u32, u32, u32, u32, u32, u32), values: [f64; 4], utc: bool) -> f64 {
    let make = if utc {
        chrono_utils::make_date_ms
    } else {
        chrono_utils::make_local_ms
    };
    make(
        parts.0 as f64,
        (parts.1 - 1) as f64,
        parts.2 as f64,
        values[0],
        values[1],
        values[2],
        values[3],
    )
}

fn argument(arguments: &[Value], index: usize, default: f64) -> Result<f64, VmError> {
    arguments
        .get(index)
        .map(crate::conversion::to_number)
        .transpose()
        .map(|value| value.unwrap_or(default))
}

pub(crate) fn time_value(receiver: Option<&Value>) -> Result<f64, VmError> {
    let valid = receiver.and_then(|value| match value {
        Value::Object(properties) => properties
            .iter()
            .any(|(name, _)| name == "timeValue")
            .then_some(value),
        _ => None,
    });
    valid.map(|value| extract_time(Some(value))).ok_or_else(|| {
        crate::value::error::throw_type_error("Date method called on incompatible receiver")
    })
}

fn local_time(parts: (i32, u32, u32, u32, u32, u32, u32), year: f64, month: f64, day: f64) -> f64 {
    chrono_utils::make_local_ms(
        year,
        month,
        day,
        parts.3 as f64,
        parts.4 as f64,
        parts.5 as f64,
        parts.6 as f64,
    )
}

fn utc_time(parts: (i32, u32, u32, u32, u32, u32, u32), year: f64, month: f64, day: f64) -> f64 {
    chrono_utils::make_date_ms(
        year,
        month,
        day,
        parts.3 as f64,
        parts.4 as f64,
        parts.5 as f64,
        parts.6 as f64,
    )
}

fn store(receiver: Option<&Value>, value: f64) -> Result<Value, VmError> {
    let value = chrono_utils::time_clip(value);
    store_time(receiver.unwrap_or(&Value::Undefined), value);
    Ok(Value::Number(value))
}
