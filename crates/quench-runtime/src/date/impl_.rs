//! Date builtin implementation logic.
use crate::{execute::VmError, ops::Builtin, value::Value};
use chrono::Datelike;
use std::rc::Rc;

use super::{chrono_utils, extract_time, helpers, store_time, DateValue};

/// Execute Date builtin methods.
pub fn execute(
    builtin: Builtin,
    receiver: Option<&Value>,
    arguments: &[Value],
) -> Option<Result<Value, VmError>> {
    if builtin == Builtin::Date {
        return Some(date_constructor(arguments));
    }
    if builtin == Builtin::DateUTC {
        return Some(date_utc(arguments));
    }
    if let Some(result) = execute_special_setter(builtin, receiver, arguments) {
        return Some(result);
    }
    execute_date_tail(builtin, receiver, arguments)
}

fn execute_date_tail(
    builtin: Builtin,
    receiver: Option<&Value>,
    arguments: &[Value],
) -> Option<Result<Value, VmError>> {
    if let Some(result) = super::setter::set_time_components(builtin, receiver, arguments) {
        return Some(result);
    }
    execute_remaining(builtin, receiver, arguments)
}

fn execute_remaining(
    builtin: Builtin,
    receiver: Option<&Value>,
    arguments: &[Value],
) -> Option<Result<Value, VmError>> {
    if is_date_getter(builtin) {
        return Some(
            super::setter::time_value(receiver)
                .map(|_| dispatch_get(builtin, receiver).unwrap_or(Value::Undefined)),
        );
    }
    if let Some(result) = super::format::execute(builtin, receiver) {
        return Some(result);
    }
    if builtin == Builtin::DateToPrimitive {
        return Some(date_to_primitive(receiver, arguments));
    }
    if builtin == Builtin::DateToTemporalInstant {
        return Some(date_to_temporal_instant(receiver));
    }
    if builtin == Builtin::DateToJSON {
        return Some(date_to_json(receiver));
    }
    let result = match builtin {
        Builtin::DateNow => Value::Number(chrono_utils::current_time_ms()),
        Builtin::DateParse => date_parse(arguments),
        _ => {
            let val = dispatch_get(builtin, receiver)
                .or_else(|| dispatch_set(builtin, receiver, arguments));
            val?
        }
    };
    Some(Ok(result))
}

fn execute_special_setter(
    builtin: Builtin,
    receiver: Option<&Value>,
    arguments: &[Value],
) -> Option<Result<Value, VmError>> {
    let result = match builtin {
        Builtin::DateSetDate => super::setter::set_date(receiver, arguments),
        Builtin::DateSetUTCDate => super::setter::set_utc_date(receiver, arguments),
        Builtin::DateSetUTCMonth => super::setter::set_utc_month(receiver, arguments),
        Builtin::DateSetUTCFullYear => super::setter::set_utc_full_year(receiver, arguments),
        Builtin::DateSetFullYear => super::setter::set_full_year(receiver, arguments),
        Builtin::DateSetMonth => super::setter::set_month(receiver, arguments),
        Builtin::DateSetTime => super::setter::set_time(receiver, arguments),
        _ => return None,
    };
    Some(result)
}

fn is_date_getter(builtin: Builtin) -> bool {
    matches!(
        builtin,
        Builtin::DateValueOf
            | Builtin::DateGetTime
            | Builtin::DateGetFullYear
            | Builtin::DateGetMonth
            | Builtin::DateGetDate
            | Builtin::DateGetDay
            | Builtin::DateGetHours
            | Builtin::DateGetMinutes
            | Builtin::DateGetSeconds
            | Builtin::DateGetMilliseconds
            | Builtin::DateGetTimezoneOffset
            | Builtin::DateGetUTCFullYear
            | Builtin::DateGetUTCMonth
            | Builtin::DateGetUTCDate
            | Builtin::DateGetUTCDay
            | Builtin::DateGetUTCHours
            | Builtin::DateGetUTCMinutes
            | Builtin::DateGetUTCSeconds
            | Builtin::DateGetUTCMilliseconds
            | Builtin::DateGetYear
    )
}

pub(crate) fn call() -> Value {
    let value = match date_constructor(&[]) {
        Ok(value) => value,
        Err(_) => Value::Undefined,
    };
    date_to_string(Some(&value))
}
fn dispatch_get(builtin: Builtin, receiver: Option<&Value>) -> Option<Value> {
    match builtin {
        Builtin::DateValueOf | Builtin::DateGetTime => Some(date_value_of(receiver)),
        Builtin::DateGetFullYear => Some(date_get_full_year(receiver)),
        Builtin::DateGetMonth => Some(date_get_month(receiver)),
        Builtin::DateGetDate => Some(date_get_date(receiver)),
        Builtin::DateGetDay => Some(date_get_day(receiver)),
        Builtin::DateGetHours => Some(date_get_hours(receiver)),
        Builtin::DateGetMinutes => Some(date_get_minutes(receiver)),
        Builtin::DateGetSeconds => Some(date_get_seconds(receiver)),
        Builtin::DateGetMilliseconds => Some(date_get_milliseconds(receiver)),
        Builtin::DateGetTimezoneOffset => Some(date_get_timezone_offset(receiver)),
        Builtin::DateGetUTCFullYear => Some(date_get_utc_full_year(receiver)),
        Builtin::DateGetUTCMonth => Some(date_get_utc_month(receiver)),
        Builtin::DateGetUTCDate => Some(date_get_utc_date(receiver)),
        Builtin::DateGetUTCDay => Some(date_get_utc_day(receiver)),
        Builtin::DateGetUTCHours => Some(date_get_utc_hours(receiver)),
        Builtin::DateGetUTCMinutes => Some(date_get_utc_minutes(receiver)),
        Builtin::DateGetUTCSeconds => Some(date_get_utc_seconds(receiver)),
        Builtin::DateGetUTCMilliseconds => Some(date_get_utc_milliseconds(receiver)),
        Builtin::DateGetYear => Some(date_get_year(receiver)),
        _ => None,
    }
}

fn dispatch_set(builtin: Builtin, receiver: Option<&Value>, args: &[Value]) -> Option<Value> {
    match builtin {
        Builtin::DateSetYear => Some(date_set_year(receiver, args)),
        _ => None,
    }
}

fn date_constructor(arguments: &[Value]) -> Result<Value, VmError> {
    let ms = if arguments.is_empty() {
        chrono_utils::time_clip(chrono_utils::current_time_ms())
    } else if arguments.len() == 1 {
        date_constructor_value(&arguments[0])?
    } else {
        let year = date_argument(arguments, 0, 0.0)?;
        let month = date_argument(arguments, 1, 0.0)?;
        let day = date_argument(arguments, 2, 1.0)?;
        let hour = date_argument(arguments, 3, 0.0)?;
        let minute = date_argument(arguments, 4, 0.0)?;
        let second = date_argument(arguments, 5, 0.0)?;
        let ms_val = date_argument(arguments, 6, 0.0)?;
        let year = chrono_utils::normalize_constructor_year(year);
        chrono_utils::make_local_ms(year, month, day, hour, minute, second, ms_val)
    };
    let props = vec![
        ("timeValue".to_string(), super::time_property(ms)),
        (
            "\0prototype".to_string(),
            Value::Builtin(Builtin::DatePrototype),
        ),
    ];
    Ok(Value::Object(Rc::new(crate::value::ObjectData::new(props))))
}

fn date_constructor_value(value: &Value) -> Result<f64, VmError> {
    if matches!(value, Value::Object(_)) && !extract_time(Some(value)).is_nan() {
        return Ok(extract_time(Some(value)));
    }
    let value = crate::conversion::to_primitive(value, "default")?;
    let time = match value {
        Value::String(string) => DateValue::parse(&string),
        value => crate::conversion::to_number(&value)?,
    };
    Ok(chrono_utils::time_clip(time))
}

fn date_parse(arguments: &[Value]) -> Value {
    let s = arguments
        .first()
        .map(helpers::value_to_string)
        .unwrap_or_default();
    Value::Number(chrono_utils::time_clip(DateValue::parse(&s)))
}

fn date_utc(arguments: &[Value]) -> Result<Value, VmError> {
    let year = date_argument(arguments, 0, f64::NAN)?;
    let month = date_argument(arguments, 1, 0.0)?;
    let day = date_argument(arguments, 2, 1.0)?;
    let hour = date_argument(arguments, 3, 0.0)?;
    let minute = date_argument(arguments, 4, 0.0)?;
    let second = date_argument(arguments, 5, 0.0)?;
    let millisecond = date_argument(arguments, 6, 0.0)?;
    Ok(Value::Number(
        DateValue::utc(year, month, day, hour, minute, second, millisecond).ms,
    ))
}

fn date_argument(arguments: &[Value], index: usize, default: f64) -> Result<f64, VmError> {
    arguments
        .get(index)
        .map(crate::conversion::to_number)
        .transpose()
        .map(|value| value.unwrap_or(default))
}

fn date_to_string(receiver: Option<&Value>) -> Value {
    Value::String(super::format::date_string(extract_time(receiver)))
}

fn date_to_primitive(receiver: Option<&Value>, arguments: &[Value]) -> Result<Value, VmError> {
    let receiver = receiver
        .filter(|value| crate::value::is_object(value))
        .ok_or_else(|| {
            crate::value::error::throw_type_error(
                "Date.prototype[Symbol.toPrimitive] requires an object",
            )
        })?;
    let hint = match arguments.first() {
        Some(Value::String(value)) if value == "string" || value == "default" => "string",
        Some(Value::String(value)) if value == "number" => "number",
        _ => return Err(crate::value::error::throw_type_error("Invalid hint")),
    };
    crate::conversion::ordinary_to_primitive(receiver, hint)
}

fn date_to_temporal_instant(receiver: Option<&Value>) -> Result<Value, VmError> {
    let time = super::setter::time_value(receiver)?;
    if time.is_nan() {
        return Err(crate::value::error::throw_range_error("Invalid time value"));
    }
    let nanoseconds = format!("{time:.0}")
        .parse::<num_bigint::BigInt>()
        .map_err(|_| crate::value::error::throw_range_error("Invalid time value"))?
        * 1_000_000_u32;
    Ok(Value::Object(Rc::new(crate::value::ObjectData::new(vec![
        (
            "epochNanoseconds".to_string(),
            Value::BigInt(nanoseconds.to_string()),
        ),
    ]))))
}

fn date_to_json(receiver: Option<&Value>) -> Result<Value, VmError> {
    let receiver = receiver
        .filter(|value| !matches!(value, Value::Null | Value::Undefined))
        .ok_or_else(|| {
            crate::value::error::throw_type_error("Cannot convert undefined or null to object")
        })?;
    let primitive = crate::conversion::to_primitive(receiver, "number")?;
    if matches!(primitive, Value::Number(number) if !number.is_finite()) {
        return Ok(Value::Null);
    }
    let method = crate::execute::get_property_result(receiver, "toISOString")?;
    crate::functions::execute_target(&method, receiver, &[])
}

fn date_value_of(receiver: Option<&Value>) -> Value {
    let ms = extract_time(receiver);
    if ms.is_nan() {
        Value::Number(f64::NAN)
    } else {
        let clipped = ms.trunc();
        Value::Number(if clipped == 0.0 { 0.0 } else { clipped })
    }
}

fn date_get_full_year(receiver: Option<&Value>) -> Value {
    chrono_utils::local_components(extract_time(receiver))
        .map(|(y, _, _, _, _, _, _)| Value::Number(y as f64))
        .unwrap_or(Value::Number(f64::NAN))
}

fn date_get_month(receiver: Option<&Value>) -> Value {
    chrono_utils::local_components(extract_time(receiver))
        .map(|(_, m, _, _, _, _, _)| Value::Number((m - 1) as f64))
        .unwrap_or(Value::Number(f64::NAN))
}

fn date_get_date(receiver: Option<&Value>) -> Value {
    chrono_utils::local_components(extract_time(receiver))
        .map(|(_, _, d, _, _, _, _)| Value::Number(d as f64))
        .unwrap_or(Value::Number(f64::NAN))
}

fn date_get_day(receiver: Option<&Value>) -> Value {
    let ms = extract_time(receiver);
    chrono_utils::local_components(ms)
        .and_then(|_| {
            let offset = chrono::Duration::minutes(chrono_utils::local_tz_offset_minutes() as i64);
            let dt = chrono_utils::ms_to_datetime(ms)?;
            Some(Value::Number(
                (dt + offset).weekday().num_days_from_sunday() as f64,
            ))
        })
        .unwrap_or(Value::Number(f64::NAN))
}

fn date_get_hours(receiver: Option<&Value>) -> Value {
    chrono_utils::local_components(extract_time(receiver))
        .map(|(_, _, _, h, _, _, _)| Value::Number(h as f64))
        .unwrap_or(Value::Number(f64::NAN))
}

fn date_get_minutes(receiver: Option<&Value>) -> Value {
    chrono_utils::local_components(extract_time(receiver))
        .map(|(_, _, _, _, m, _, _)| Value::Number(m as f64))
        .unwrap_or(Value::Number(f64::NAN))
}

fn date_get_seconds(receiver: Option<&Value>) -> Value {
    chrono_utils::local_components(extract_time(receiver))
        .map(|(_, _, _, _, _, s, _)| Value::Number(s as f64))
        .unwrap_or(Value::Number(f64::NAN))
}

fn date_get_milliseconds(receiver: Option<&Value>) -> Value {
    chrono_utils::local_components(extract_time(receiver))
        .map(|(_, _, _, _, _, _, ms)| Value::Number(ms as f64))
        .unwrap_or(Value::Number(f64::NAN))
}

fn date_get_timezone_offset(receiver: Option<&Value>) -> Value {
    if extract_time(receiver).is_nan() {
        Value::Number(f64::NAN)
    } else {
        Value::Number(-f64::from(chrono_utils::local_tz_offset_minutes()))
    }
}

fn date_get_utc_full_year(receiver: Option<&Value>) -> Value {
    chrono_utils::utc_components(extract_time(receiver))
        .map(|(y, _, _, _, _, _, _)| Value::Number(y as f64))
        .unwrap_or(Value::Number(f64::NAN))
}

fn date_get_utc_month(receiver: Option<&Value>) -> Value {
    chrono_utils::utc_components(extract_time(receiver))
        .map(|(_, m, _, _, _, _, _)| Value::Number((m - 1) as f64))
        .unwrap_or(Value::Number(f64::NAN))
}

fn date_get_utc_date(receiver: Option<&Value>) -> Value {
    chrono_utils::utc_components(extract_time(receiver))
        .map(|(_, _, d, _, _, _, _)| Value::Number(d as f64))
        .unwrap_or(Value::Number(f64::NAN))
}

fn date_get_utc_day(receiver: Option<&Value>) -> Value {
    chrono_utils::utc_components(extract_time(receiver))
        .and_then(|(y, m, d, _, _, _, _)| {
            let ndt = chrono::NaiveDate::from_ymd_opt(y, m, d)?;
            Some(Value::Number(ndt.weekday().num_days_from_sunday() as f64))
        })
        .unwrap_or(Value::Number(f64::NAN))
}

fn date_get_utc_hours(receiver: Option<&Value>) -> Value {
    chrono_utils::utc_components(extract_time(receiver))
        .map(|(_, _, _, h, _, _, _)| Value::Number(h as f64))
        .unwrap_or(Value::Number(f64::NAN))
}

fn date_get_utc_minutes(receiver: Option<&Value>) -> Value {
    chrono_utils::utc_components(extract_time(receiver))
        .map(|(_, _, _, _, m, _, _)| Value::Number(m as f64))
        .unwrap_or(Value::Number(f64::NAN))
}

fn date_get_utc_seconds(receiver: Option<&Value>) -> Value {
    chrono_utils::utc_components(extract_time(receiver))
        .map(|(_, _, _, _, _, s, _)| Value::Number(s as f64))
        .unwrap_or(Value::Number(f64::NAN))
}

fn date_get_utc_milliseconds(receiver: Option<&Value>) -> Value {
    chrono_utils::utc_components(extract_time(receiver))
        .map(|(_, _, _, _, _, _, ms)| Value::Number(ms as f64))
        .unwrap_or(Value::Number(f64::NAN))
}

fn date_get_year(receiver: Option<&Value>) -> Value {
    let year = chrono_utils::local_components(extract_time(receiver))
        .map(|(y, _, _, _, _, _, _)| y as f64 - 1900.0)
        .unwrap_or(f64::NAN);
    Value::Number(if year.is_nan() { f64::NAN } else { year })
}

fn date_set_year(receiver: Option<&Value>, arguments: &[Value]) -> Value {
    let year = helpers::to_int32(arguments.first().unwrap_or(&Value::Undefined));
    let year = if (0.0..=99.0).contains(&year) {
        year + 1900.0
    } else {
        year
    };
    let current = extract_time(receiver);
    let (_, m, d, h, min, s, ms) =
        chrono_utils::local_components(current).unwrap_or((2000, 1, 1, 0, 0, 0, 0));
    let result = chrono_utils::make_local_ms(
        year,
        (m - 1) as f64,
        d as f64,
        h as f64,
        min as f64,
        s as f64,
        ms as f64,
    );
    store_time(receiver.unwrap_or(&Value::Undefined), result);
    Value::Number(chrono_utils::time_clip(result))
}
