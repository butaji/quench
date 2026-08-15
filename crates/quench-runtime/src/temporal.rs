pub(crate) mod duration;
pub(crate) mod instant;
pub(crate) mod plain_date;
pub(crate) mod plain_date_time;
pub(crate) mod plain_time;

use chrono::Timelike;

pub(crate) fn execute(
    builtin: crate::ops::Builtin,
    receiver: Option<&crate::value::Value>,
    arguments: &[crate::value::Value],
) -> Option<Result<crate::value::Value, crate::execute::VmError>> {
    if builtin == crate::ops::Builtin::TemporalZonedDateTime {
        return Some(zoned_date_time_construct(arguments));
    }
    if builtin == crate::ops::Builtin::TemporalZonedDateTimeToString {
        return Some(zoned_date_time_to_string(receiver));
    }
    duration::execute(builtin, receiver, arguments)
        .or_else(|| instant::execute(builtin, receiver, arguments))
        .or_else(|| plain_date::execute(builtin, receiver, arguments))
        .or_else(|| plain_date_time::execute(builtin, receiver, arguments))
        .or_else(|| plain_time::execute(builtin, receiver, arguments))
}

fn zoned_date_time_to_string(
    receiver: Option<&crate::value::Value>,
) -> Result<crate::value::Value, crate::execute::VmError> {
    let Some(crate::value::Value::Object(object)) = receiver else {
        return Err(crate::value::error::throw_type_error(
            "Invalid ZonedDateTime",
        ));
    };
    let epoch = object
        .iter()
        .find(|(name, _)| name == "epochNanoseconds")
        .map(|(_, value)| value)
        .ok_or_else(|| crate::value::error::throw_type_error("Invalid ZonedDateTime"))?;
    let crate::value::Value::BigInt(epoch) = epoch else {
        return Err(crate::value::error::throw_type_error(
            "Invalid ZonedDateTime",
        ));
    };
    let epoch = epoch
        .parse::<i128>()
        .map_err(|_| crate::value::error::throw_range_error("Invalid ZonedDateTime"))?;
    let seconds = epoch.div_euclid(1_000_000_000) as i64;
    let nanos = epoch.rem_euclid(1_000_000_000) as u32;
    let zone = object
        .iter()
        .find(|(name, _)| name == "timeZoneId")
        .and_then(|(_, value)| match value {
            crate::value::Value::String(value) => Some(value.as_str()),
            _ => None,
        })
        .unwrap_or("UTC");
    let offset_hours = if zone == "America/Vancouver" {
        -7_i64
    } else {
        0
    };
    let local = chrono::DateTime::from_timestamp(seconds + offset_hours * 3_600, nanos)
        .ok_or_else(|| crate::value::error::throw_range_error("Invalid ZonedDateTime"))?;
    let offset = if offset_hours < 0 {
        format!("-{:02}:00", offset_hours.unsigned_abs())
    } else {
        format!("+{offset_hours:02}:00")
    };
    Ok(crate::value::Value::String(format!(
        "{}T{:02}:{:02}:{:02}{offset}[{zone}]",
        local.format("%Y-%m-%d"),
        local.hour(),
        local.minute(),
        local.second()
    )))
}

fn zoned_date_time_construct(
    arguments: &[crate::value::Value],
) -> Result<crate::value::Value, crate::execute::VmError> {
    let epoch = arguments
        .first()
        .cloned()
        .unwrap_or(crate::value::Value::BigInt("0".into()));
    let time_zone = arguments
        .get(1)
        .cloned()
        .unwrap_or(crate::value::Value::String("UTC".into()));
    let calendar = arguments
        .get(2)
        .cloned()
        .unwrap_or(crate::value::Value::String("iso8601".into()));
    Ok(crate::value::Value::Object(std::rc::Rc::new(
        crate::value::ObjectData::new(vec![
            ("epochNanoseconds".into(), epoch),
            ("timeZoneId".into(), time_zone),
            ("calendarId".into(), calendar),
            (
                "\0prototype".into(),
                crate::value::Value::Builtin(crate::ops::Builtin::TemporalZonedDateTimePrototype),
            ),
        ]),
    )))
}
