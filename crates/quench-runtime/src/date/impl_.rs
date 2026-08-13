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
    if builtin == Builtin::DateSetDate {
        return Some(date_set_date(receiver, arguments));
    }
    let result = match builtin {
        Builtin::DateNow => Value::Number(chrono_utils::current_time_ms()),
        Builtin::DateParse => date_parse(arguments),
        Builtin::DateToString => date_to_string(receiver),
        _ => {
            let val = dispatch_get(builtin, receiver)
                .or_else(|| dispatch_set(builtin, receiver, arguments));
            val?
        }
    };
    Some(Ok(result))
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
        Builtin::DateSetTime => Some(date_set_time(receiver, args)),
        Builtin::DateSetFullYear => Some(date_set_full_year(receiver, args)),
        Builtin::DateSetMonth => Some(date_set_month(receiver, args)),
        Builtin::DateSetHours => Some(date_set_hours(receiver, args)),
        Builtin::DateSetMinutes => Some(date_set_minutes(receiver, args)),
        Builtin::DateSetSeconds => Some(date_set_seconds(receiver, args)),
        Builtin::DateSetMilliseconds => Some(date_set_milliseconds(receiver, args)),
        Builtin::DateSetUTCHours => Some(date_set_utc_hours(receiver, args)),
        Builtin::DateSetUTCMinutes => Some(date_set_utc_minutes(receiver, args)),
        Builtin::DateSetUTCSeconds => Some(date_set_utc_seconds(receiver, args)),
        Builtin::DateSetUTCMilliseconds => Some(date_set_utc_milliseconds(receiver, args)),
        Builtin::DateSetYear => Some(date_set_year(receiver, args)),
        _ => None,
    }
}

fn date_constructor(arguments: &[Value]) -> Result<Value, VmError> {
    let ms = if arguments.is_empty() {
        chrono_utils::time_clip(chrono_utils::current_time_ms())
    } else if arguments.len() == 1 {
        let val = &arguments[0];
        if let Value::String(s) = val {
            chrono_utils::time_clip(DateValue::parse(s))
        } else {
            chrono_utils::time_clip(crate::conversion::to_number(val)?)
        }
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
    let props = vec![("timeValue".to_string(), super::time_property(ms))];
    Ok(Value::Object(Rc::new(crate::value::ObjectData::new(props))))
}

fn date_parse(arguments: &[Value]) -> Value {
    let s = arguments
        .first()
        .map(helpers::value_to_string)
        .unwrap_or_default();
    Value::Number(chrono_utils::time_clip(DateValue::parse(&s)))
}

fn date_utc(arguments: &[Value]) -> Result<Value, VmError> {
    let year = date_argument(arguments, 0, 0.0)?;
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
    Value::String(format_date(extract_time(receiver)))
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

fn date_set_time(receiver: Option<&Value>, arguments: &[Value]) -> Value {
    let ms = helpers::to_number(arguments.first().unwrap_or(&Value::Undefined));
    store_time(receiver.unwrap_or(&Value::Undefined), ms);
    Value::Number(chrono_utils::time_clip(ms))
}

fn date_set_full_year(receiver: Option<&Value>, arguments: &[Value]) -> Value {
    let current = extract_time(receiver);
    let (cy, cm, cd, ch, cmin, cs, cms) =
        chrono_utils::local_components(current).unwrap_or((2000, 1, 1, 0, 0, 0, 0));
    let year = helpers::arg_or(arguments.first(), cy as f64);
    let month = helpers::arg_or(arguments.get(1), (cm - 1) as f64);
    let day = helpers::arg_or(arguments.get(2), cd as f64);
    let hour = helpers::arg_or(arguments.get(3), ch as f64);
    let minute = helpers::arg_or(arguments.get(4), cmin as f64);
    let second = helpers::arg_or(arguments.get(5), cs as f64);
    let ms_val = helpers::arg_or(arguments.get(6), cms as f64);
    let result = chrono_utils::make_local_ms(year, month, day, hour, minute, second, ms_val);
    store_time(receiver.unwrap_or(&Value::Undefined), result);
    Value::Number(chrono_utils::time_clip(result))
}

fn date_set_month(receiver: Option<&Value>, arguments: &[Value]) -> Value {
    let current = extract_time(receiver);
    let new_month = helpers::to_int32(arguments.first().unwrap_or(&Value::Undefined));
    let (y, _, d, h, m, s, ms) =
        chrono_utils::local_components(current).unwrap_or((2000, 1, 1, 0, 0, 0, 0));
    let result = chrono_utils::make_local_ms(
        y as f64, new_month, d as f64, h as f64, m as f64, s as f64, ms as f64,
    );
    store_time(receiver.unwrap_or(&Value::Undefined), result);
    Value::Number(chrono_utils::time_clip(result))
}

fn date_set_date(receiver: Option<&Value>, arguments: &[Value]) -> Result<Value, VmError> {
    let current = date_time(receiver)?;
    let day = date_argument(arguments, 0, f64::NAN)?;
    let result =
        chrono_utils::local_components(current).map_or(f64::NAN, |(y, m, _, h, min, s, ms)| {
            chrono_utils::make_local_ms(
                y as f64,
                (m - 1) as f64,
                day,
                h as f64,
                min as f64,
                s as f64,
                ms as f64,
            )
        });
    let result = chrono_utils::time_clip(result);
    store_time(receiver.unwrap_or(&Value::Undefined), result);
    Ok(Value::Number(result))
}

fn date_time(receiver: Option<&Value>) -> Result<f64, VmError> {
    let value = receiver.filter(|value| matches!(value, Value::Object(_)));
    let valid = value.and_then(|value| match value {
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

fn date_set_hours(receiver: Option<&Value>, arguments: &[Value]) -> Value {
    set_local_time_components(receiver, arguments, 0)
}

fn date_set_minutes(receiver: Option<&Value>, arguments: &[Value]) -> Value {
    set_local_time_components(receiver, arguments, 1)
}

fn date_set_seconds(receiver: Option<&Value>, arguments: &[Value]) -> Value {
    set_local_time_components(receiver, arguments, 2)
}

fn date_set_milliseconds(receiver: Option<&Value>, arguments: &[Value]) -> Value {
    set_local_time_components(receiver, arguments, 3)
}

fn date_set_utc_hours(receiver: Option<&Value>, arguments: &[Value]) -> Value {
    set_utc_time_components(receiver, arguments, 0)
}

fn date_set_utc_minutes(receiver: Option<&Value>, arguments: &[Value]) -> Value {
    set_utc_time_components(receiver, arguments, 1)
}

fn date_set_utc_seconds(receiver: Option<&Value>, arguments: &[Value]) -> Value {
    set_utc_time_components(receiver, arguments, 2)
}

fn date_set_utc_milliseconds(receiver: Option<&Value>, arguments: &[Value]) -> Value {
    set_utc_time_components(receiver, arguments, 3)
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

fn settable_components(arguments: &[Value], start: usize, current: [f64; 4]) -> [f64; 4] {
    let mut out = current;
    for (slot, value) in out
        .iter_mut()
        .enumerate()
        .filter(|(slot, _)| *slot >= start)
    {
        let index = slot - start;
        if index < arguments.len() {
            *value = helpers::to_int32(&arguments[index]);
        }
    }
    out
}

fn set_local_time_components(receiver: Option<&Value>, arguments: &[Value], start: usize) -> Value {
    let current = extract_time(receiver);
    let (y, m, d, ch, cm, cs, cms) =
        chrono_utils::local_components(current).unwrap_or((2000, 1, 1, 0, 0, 0, 0));
    let [hour, minute, second, ms_val] = settable_components(
        arguments,
        start,
        [ch as f64, cm as f64, cs as f64, cms as f64],
    );
    let result = chrono_utils::make_local_ms(
        y as f64,
        (m - 1) as f64,
        d as f64,
        hour,
        minute,
        second,
        ms_val,
    );
    store_time(receiver.unwrap_or(&Value::Undefined), result);
    Value::Number(chrono_utils::time_clip(result))
}

fn set_utc_time_components(receiver: Option<&Value>, arguments: &[Value], start: usize) -> Value {
    let current = extract_time(receiver);
    let (y, m, d, ch, cm, cs, cms) =
        chrono_utils::utc_components(current).unwrap_or((2000, 1, 1, 0, 0, 0, 0));
    let [hour, minute, second, ms_val] = settable_components(
        arguments,
        start,
        [ch as f64, cm as f64, cs as f64, cms as f64],
    );
    let result = chrono_utils::make_date_ms(
        y as f64,
        (m - 1) as f64,
        d as f64,
        hour,
        minute,
        second,
        ms_val,
    );
    store_time(receiver.unwrap_or(&Value::Undefined), result);
    Value::Number(chrono_utils::time_clip(result))
}

fn format_date(ms: f64) -> String {
    if ms.is_nan() || ms.is_infinite() {
        return "Invalid Date".to_string();
    }
    let Some((year, month, day, hour, minute, second, _)) = chrono_utils::local_components(ms)
    else {
        return "Invalid Date".to_string();
    };
    let tz_offset = chrono_utils::local_tz_offset_minutes();
    let tz_hours = tz_offset.abs() / 60;
    let tz_mins = (tz_offset.abs() % 60) as u32;
    let tz_sign = if tz_offset >= 0 { "+" } else { "-" };

    let day_names = ["Sun", "Mon", "Tue", "Wed", "Thu", "Fri", "Sat"];
    let month_names = [
        "Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec",
    ];
    let weekday = chrono_utils::ms_to_datetime(ms)
        .map(|dt| {
            let offset = chrono::Duration::minutes(tz_offset as i64);
            (dt + offset).weekday().num_days_from_sunday() as usize
        })
        .unwrap_or(0);

    format!(
        "{} {} {:02} {} {:02}:{:02}:{:02} GMT{}{:02}{:02}",
        day_names[weekday],
        month_names[(month - 1) as usize],
        day,
        year,
        hour,
        minute,
        second,
        tz_sign,
        tz_hours,
        tz_mins
    )
}
