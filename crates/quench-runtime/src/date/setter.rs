//! Completion-aware Date setters.

use crate::{execute::VmError, value::Value};

use super::{chrono_utils, extract_time, store_time};

pub fn set_date(receiver: Option<&Value>, arguments: &[Value]) -> Result<Value, VmError> {
    let current = date_time(receiver)?;
    let day = argument(arguments, 0, f64::NAN)?;
    let result = chrono_utils::local_components(current).map_or(f64::NAN, |parts| {
        local_time(parts, parts.0 as f64, (parts.1 - 1) as f64, day)
    });
    store(receiver, result)
}

pub fn set_full_year(receiver: Option<&Value>, arguments: &[Value]) -> Result<Value, VmError> {
    let current = date_time(receiver)?;
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

fn argument(arguments: &[Value], index: usize, default: f64) -> Result<f64, VmError> {
    arguments
        .get(index)
        .map(crate::conversion::to_number)
        .transpose()
        .map(|value| value.unwrap_or(default))
}

fn date_time(receiver: Option<&Value>) -> Result<f64, VmError> {
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

fn store(receiver: Option<&Value>, value: f64) -> Result<Value, VmError> {
    let value = chrono_utils::time_clip(value);
    store_time(receiver.unwrap_or(&Value::Undefined), value);
    Ok(Value::Number(value))
}
