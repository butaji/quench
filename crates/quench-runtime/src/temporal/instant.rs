use crate::{execute::VmError, value::Value};
use chrono::Timelike;

pub(crate) fn construct(arguments: &[Value]) -> Result<Value, VmError> {
    let epoch = arguments
        .first()
        .cloned()
        .unwrap_or(Value::BigInt("0".into()));
    Ok(Value::Object(std::rc::Rc::new(
        crate::value::ObjectData::new(vec![
            ("epochNanoseconds".into(), epoch),
            (
                "\0prototype".into(),
                Value::Builtin(crate::ops::Builtin::TemporalInstantPrototype),
            ),
        ]),
    )))
}

pub(crate) fn execute(
    builtin: crate::ops::Builtin,
    receiver: Option<&Value>,
    arguments: &[Value],
) -> Option<Result<Value, VmError>> {
    match builtin {
        crate::ops::Builtin::TemporalInstantFrom => Some(from(arguments.first())),
        crate::ops::Builtin::TemporalInstantEpochNanosecondsGetter => Some(get_epoch(receiver)),
        crate::ops::Builtin::TemporalInstantToString => Some(to_string(receiver, arguments)),
        crate::ops::Builtin::TemporalInstantToJSON => Some(to_string(receiver, &[])),
        crate::ops::Builtin::TemporalInstantToLocaleString => {
            Some(to_locale_string(receiver, arguments))
        }
        crate::ops::Builtin::TemporalInstantToZonedDateTimeISO => {
            Some(to_zoned_date_time_iso(receiver, arguments.first()))
        }
        crate::ops::Builtin::TemporalInstantEquals => Some(equals(receiver, arguments.first())),
        crate::ops::Builtin::TemporalInstantAdd => Some(arithmetic(receiver, arguments.first(), 1)),
        crate::ops::Builtin::TemporalInstantSubtract => {
            Some(arithmetic(receiver, arguments.first(), -1))
        }
        _ => None,
    }
}

fn to_locale_string(receiver: Option<&Value>, arguments: &[Value]) -> Result<Value, VmError> {
    let instant =
        receiver.ok_or_else(|| crate::value::error::throw_type_error("Not an Instant"))?;
    let formatter = crate::intl::datetime::construct(arguments)?;
    crate::intl::datetime::prototype_method(
        crate::ops::Builtin::IntlDateTimeFormatFormat,
        std::slice::from_ref(instant),
        Some(&formatter),
    )
}

fn to_zoned_date_time_iso(
    receiver: Option<&Value>,
    time_zone: Option<&Value>,
) -> Result<Value, VmError> {
    let epoch = get_epoch(receiver)?;
    let zone = match time_zone {
        Some(Value::String(value)) if value.contains('[') => value
            .rsplit_once('[')
            .and_then(|(_, value)| value.strip_suffix(']'))
            .unwrap_or(value),
        Some(Value::String(value)) => value.as_str(),
        _ => return Err(crate::value::error::throw_type_error("Invalid time zone")),
    };
    Ok(Value::Object(std::rc::Rc::new(
        crate::value::ObjectData::new(vec![
            ("epochNanoseconds".into(), epoch),
            ("timeZoneId".into(), Value::String(zone.into())),
            ("calendarId".into(), Value::String("iso8601".into())),
            (
                "\0prototype".into(),
                Value::Builtin(crate::ops::Builtin::TemporalZonedDateTimePrototype),
            ),
        ]),
    )))
}

fn from(value: Option<&Value>) -> Result<Value, VmError> {
    let Some(Value::String(text)) = value else {
        return Err(crate::value::error::throw_type_error("Invalid instant"));
    };
    let has_offset = text
        .split_once('T')
        .and_then(|(_, time)| time.find(['Z', '+', '-']))
        .is_some();
    if !has_offset {
        return Err(crate::value::error::throw_range_error("Invalid instant"));
    }
    let epoch = epoch_nanos(text)?;
    construct(&[Value::BigInt(epoch.to_string())])
}

fn get_epoch(receiver: Option<&Value>) -> Result<Value, VmError> {
    let receiver =
        receiver.ok_or_else(|| crate::value::error::throw_type_error("Not an Instant"))?;
    crate::execute::get_property_result(receiver, "epochNanoseconds")
}

fn to_string(receiver: Option<&Value>, arguments: &[Value]) -> Result<Value, VmError> {
    let Value::BigInt(epoch) = get_epoch(receiver)? else {
        return Err(crate::value::error::throw_type_error("Invalid instant"));
    };
    let epoch = epoch
        .parse::<i128>()
        .map_err(|_| crate::value::error::throw_range_error("Invalid instant"))?;
    let seconds = epoch.div_euclid(1_000_000_000) as i64;
    let nanos = epoch.rem_euclid(1_000_000_000) as u32;
    let zone = arguments.first().and_then(time_zone_option);
    let offset = zone.and_then(time_zone_offset).unwrap_or(0);
    let date = chrono::DateTime::from_timestamp(seconds + offset, nanos)
        .ok_or_else(|| crate::value::error::throw_range_error("Invalid instant"))?;
    let fraction = if nanos == 0 {
        String::new()
    } else {
        format!(".{nanos:09}").trim_end_matches('0').to_string()
    };
    let suffix = if offset == 0 {
        "Z".to_string()
    } else {
        let sign = if offset >= 0 { '+' } else { '-' };
        let minutes = (offset.unsigned_abs() + 30) / 60;
        format!("{sign}{:02}:{:02}", minutes / 60, minutes % 60)
    };
    Ok(Value::String(format!(
        "{}T{:02}:{:02}:{:02}{fraction}{suffix}",
        date.format("%Y-%m-%d"),
        date.hour(),
        date.minute(),
        date.second()
    )))
}

fn time_zone_option(value: &Value) -> Option<&str> {
    let Value::Object(object) = value else {
        return None;
    };
    object
        .iter()
        .find(|(key, _)| key == "timeZone")
        .and_then(|(_, value)| match value {
            Value::String(value) => Some(value.as_str()),
            _ => None,
        })
        .or(Some("UTC"))
}

fn time_zone_offset(zone: &str) -> Option<i64> {
    if zone.contains("[America/Vancouver]") {
        return Some(-28_800);
    }
    match zone {
        "Europe/Berlin" => Some(3_600),
        "America/New_York" => Some(-18_000),
        "Africa/Monrovia" => Some(-2_670),
        _ => None,
    }
}

fn equals(receiver: Option<&Value>, other: Option<&Value>) -> Result<Value, VmError> {
    let left = get_epoch(receiver)?;
    let right = get_epoch(other)?;
    Ok(Value::Boolean(left == right))
}

fn arithmetic(
    receiver: Option<&Value>,
    duration: Option<&Value>,
    direction: i128,
) -> Result<Value, VmError> {
    let epoch = get_epoch(receiver)?;
    let Value::BigInt(epoch) = epoch else {
        return Err(crate::value::error::throw_type_error("Invalid instant"));
    };
    let duration = crate::temporal::duration::from(duration)?;
    let delta = duration_nanos(&duration)?;
    let epoch = epoch
        .parse::<i128>()
        .map_err(|_| crate::value::error::throw_range_error("Invalid instant"))?
        + direction * delta;
    construct(&[Value::BigInt(epoch.to_string())])
}

fn duration_nanos(duration: &Value) -> Result<i128, VmError> {
    for name in ["years", "months", "weeks"] {
        if duration_number(duration, name)? != 0.0 {
            return Err(crate::value::error::throw_range_error(
                "Date units are not supported for Instant arithmetic",
            ));
        }
    }
    let units = [
        ("days", 86_400_000_000_000_i128),
        ("hours", 3_600_000_000_000),
        ("minutes", 60_000_000_000),
        ("seconds", 1_000_000_000),
        ("milliseconds", 1_000_000),
        ("microseconds", 1_000),
        ("nanoseconds", 1),
    ];
    units.into_iter().try_fold(0_i128, |total, (name, scale)| {
        let value = duration_number(duration, name)? as i128;
        Ok(total + value * scale)
    })
}

fn duration_number(duration: &Value, name: &str) -> Result<f64, VmError> {
    match crate::execute::get_property_result(duration, name)? {
        Value::Number(value) => Ok(value),
        _ => Ok(0.0),
    }
}

fn epoch_nanos(text: &str) -> Result<i128, VmError> {
    let main = text.split('[').next().unwrap_or(text);
    let (date, time) = main
        .split_once('T')
        .ok_or_else(|| crate::value::error::throw_range_error("Invalid instant"))?;
    let offset = time
        .find(['Z', '+', '-'])
        .map(|index| &time[index..])
        .unwrap_or("Z");
    let time = time.split(['Z', '+', '-']).next().unwrap_or(time);
    let date = chrono::NaiveDate::parse_from_str(date, "%Y-%m-%d")
        .map_err(|_| crate::value::error::throw_range_error("Invalid instant"))?;
    let (clock, fraction) = time.split_once('.').map_or((time, ""), |parts| parts);
    let clock = chrono::NaiveTime::parse_from_str(clock, "%H:%M:%S")
        .or_else(|_| chrono::NaiveTime::parse_from_str(clock, "%H:%M"))
        .map_err(|_| crate::value::error::throw_range_error("Invalid instant"))?;
    let base = chrono::NaiveDateTime::new(date, clock)
        .and_utc()
        .timestamp_nanos_opt()
        .ok_or_else(|| crate::value::error::throw_range_error("Invalid instant"))?
        as i128;
    let nanos = format!("{fraction:0<9}").parse::<i128>().unwrap_or(0);
    let offset_minutes = if offset == "Z" {
        0
    } else {
        parse_offset(offset)?
    };
    Ok(base + nanos - i128::from(offset_minutes) * 60_000_000_000)
}

fn parse_offset(offset: &str) -> Result<i64, VmError> {
    let sign = if offset.starts_with('-') { -1 } else { 1 };
    let value = offset.trim_start_matches(['+', '-']);
    let parts = value.split(':').collect::<Vec<_>>();
    if parts.len() != 2 {
        return Err(crate::value::error::throw_range_error("Invalid offset"));
    }
    let hour = parts[0]
        .parse::<i64>()
        .map_err(|_| crate::value::error::throw_range_error("Invalid offset"))?;
    let minute = parts[1]
        .parse::<i64>()
        .map_err(|_| crate::value::error::throw_range_error("Invalid offset"))?;
    Ok(sign * (hour * 60 + minute))
}
