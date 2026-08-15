use crate::{execute::VmError, value::Value};
use chrono::{Datelike, Duration as CalendarDuration, NaiveDate, Timelike};

const NAMES: [&str; 9] = [
    "year",
    "month",
    "day",
    "hour",
    "minute",
    "second",
    "millisecond",
    "microsecond",
    "nanosecond",
];

pub(crate) fn construct(arguments: &[Value]) -> Result<Value, VmError> {
    let mut fields = arguments
        .iter()
        .take(9)
        .map(crate::conversion::to_number)
        .collect::<Result<Vec<_>, _>>()?;
    while fields.len() < 9 {
        fields.push(0.0);
    }
    if fields[5] == 60.0 {
        fields[5] = 59.0;
    }
    validate(&fields)?;
    let month_code = format!("M{:02}", fields[1] as u32);
    let calendar = arguments
        .get(9)
        .filter(|value| matches!(value, Value::String(_)))
        .cloned()
        .unwrap_or_else(|| Value::String("iso8601".into()));
    let properties = NAMES
        .into_iter()
        .zip(fields)
        .map(|(name, value)| (name.into(), Value::Number(value)))
        .chain([
            ("monthCode".into(), Value::String(month_code)),
            ("calendarId".into(), calendar),
            (
                "\0prototype".into(),
                Value::Builtin(crate::ops::Builtin::TemporalPlainDateTimePrototype),
            ),
        ])
        .collect();
    Ok(Value::Object(std::rc::Rc::new(
        crate::value::ObjectData::new(properties),
    )))
}

fn validate(fields: &[f64]) -> Result<(), VmError> {
    if fields
        .iter()
        .any(|value| !value.is_finite() || value.fract() != 0.0)
        || !(1.0..=12.0).contains(&fields[1])
        || !(1.0..=31.0).contains(&fields[2])
        || !(0.0..=23.0).contains(&fields[3])
        || !(0.0..=59.0).contains(&fields[4])
        || !(0.0..=60.0).contains(&fields[5])
        || fields[4..]
            .iter()
            .any(|value| !(0.0..=999.0).contains(value))
    {
        return Err(crate::value::error::throw_range_error("Invalid date-time"));
    }
    if fields[0].abs() < 262_000.0
        && chrono::NaiveDate::from_ymd_opt(fields[0] as i32, fields[1] as u32, fields[2] as u32)
            .is_none()
    {
        return Err(crate::value::error::throw_range_error("Invalid date-time"));
    }
    if outside_temporal_range(fields) {
        return Err(crate::value::error::throw_range_error("Invalid date-time"));
    }
    Ok(())
}

fn outside_temporal_range(fields: &[f64]) -> bool {
    let year = fields[0] as i32;
    let date_before_min = year < -271_821
        || (year == -271_821 && (fields[1] < 4.0 || (fields[1] == 4.0 && fields[2] < 19.0)));
    let at_min_midnight = year == -271_821
        && fields[1] == 4.0
        && fields[2] == 19.0
        && fields[3..].iter().all(|value| *value == 0.0);
    let after_max = year > 275_760
        || (year == 275_760 && (fields[1] > 9.0 || (fields[1] == 9.0 && fields[2] > 13.0)));
    date_before_min || at_min_midnight || after_max
}

pub(crate) fn execute(
    builtin: crate::ops::Builtin,
    receiver: Option<&Value>,
    arguments: &[Value],
) -> Option<Result<Value, VmError>> {
    match builtin {
        crate::ops::Builtin::TemporalPlainDateTimeFrom => {
            Some(from(arguments.first(), arguments.get(1)))
        }
        crate::ops::Builtin::TemporalPlainDateTimeCalendarIdGetter
        | crate::ops::Builtin::TemporalPlainDateTimeYearGetter
        | crate::ops::Builtin::TemporalPlainDateTimeMonthGetter
        | crate::ops::Builtin::TemporalPlainDateTimeMonthCodeGetter
        | crate::ops::Builtin::TemporalPlainDateTimeDayGetter
        | crate::ops::Builtin::TemporalPlainDateTimeHourGetter
        | crate::ops::Builtin::TemporalPlainDateTimeMinuteGetter
        | crate::ops::Builtin::TemporalPlainDateTimeSecondGetter
        | crate::ops::Builtin::TemporalPlainDateTimeMillisecondGetter
        | crate::ops::Builtin::TemporalPlainDateTimeMicrosecondGetter
        | crate::ops::Builtin::TemporalPlainDateTimeNanosecondGetter => {
            Some(getter(builtin, receiver))
        }
        crate::ops::Builtin::TemporalPlainDateTimeToString
        | crate::ops::Builtin::TemporalPlainDateTimeToJSON => {
            Some(to_string(receiver, arguments.first()))
        }
        crate::ops::Builtin::TemporalPlainDateTimeToLocaleString => {
            Some(to_locale_string(receiver, arguments))
        }
        crate::ops::Builtin::TemporalPlainDateTimeCompare => Some(compare(arguments)),
        crate::ops::Builtin::TemporalPlainDateTimeEquals => {
            Some(equals(receiver, arguments.first()))
        }
        crate::ops::Builtin::TemporalPlainDateTimeValueOf => Some(Err(
            crate::value::error::throw_type_error("Cannot convert PlainDateTime to a number"),
        )),
        crate::ops::Builtin::TemporalPlainDateTimeAdd => {
            Some(add(receiver, arguments.first(), 1.0))
        }
        crate::ops::Builtin::TemporalPlainDateTimeSubtract => {
            Some(add(receiver, arguments.first(), -1.0))
        }
        crate::ops::Builtin::TemporalPlainDateTimeWith => {
            Some(with(receiver, arguments.first(), arguments.get(1)))
        }
        crate::ops::Builtin::TemporalPlainDateTimeRound => Some(round(receiver, arguments.first())),
        crate::ops::Builtin::TemporalPlainDateTimeToZonedDateTime => {
            Some(to_zoned_date_time(receiver, arguments.first()))
        }
        _ => None,
    }
}

fn to_locale_string(receiver: Option<&Value>, arguments: &[Value]) -> Result<Value, VmError> {
    let calendar =
        receiver.and_then(|value| match value {
            Value::Object(object) => object.iter().find(|(key, _)| key == "calendarId").and_then(
                |(_, value)| match value {
                    Value::String(value) => Some(value.as_str()),
                    _ => None,
                },
            ),
            _ => None,
        });
    if arguments.get(1).is_none() && !matches!(calendar, Some("iso8601") | Some("gregory") | None) {
        return Err(crate::value::error::throw_range_error("Calendar mismatch"));
    }
    if let (Some(Value::Object(object)), Some(Value::Object(options))) =
        (receiver, arguments.get(1))
    {
        if let Value::String(option_calendar) =
            crate::execute::get_property_result(&Value::Object(options.clone()), "calendar")?
        {
            let actual = object
                .iter()
                .find(|(key, _)| key == "calendarId")
                .and_then(|(_, value)| match value {
                    Value::String(value) => Some(value.as_str()),
                    _ => None,
                })
                .unwrap_or("iso8601");
            if option_calendar != actual {
                return Err(crate::value::error::throw_range_error("Calendar mismatch"));
            }
        }
    }
    let formatter = crate::intl::datetime::construct(arguments)?;
    crate::intl::datetime::prototype_method(
        crate::ops::Builtin::IntlDateTimeFormatFormat,
        &[receiver
            .cloned()
            .ok_or_else(|| crate::value::error::throw_type_error("Not a PlainDateTime"))?],
        Some(&formatter),
    )
}

fn to_zoned_date_time(
    receiver: Option<&Value>,
    time_zone: Option<&Value>,
) -> Result<Value, VmError> {
    let Some(Value::Object(object)) = receiver else {
        return Err(crate::value::error::throw_type_error(
            "Invalid PlainDateTime",
        ));
    };
    let Value::String(time_zone) = time_zone.unwrap_or(&Value::Undefined) else {
        return Err(crate::value::error::throw_type_error("Invalid time zone"));
    };
    let number = |name| object_number(object, name) as u32;
    let date =
        chrono::NaiveDate::from_ymd_opt(number("year") as i32, number("month"), number("day"))
            .ok_or_else(|| crate::value::error::throw_range_error("Invalid date-time"))?;
    let time = chrono::NaiveTime::from_hms_nano_opt(
        number("hour"),
        number("minute"),
        number("second"),
        number("millisecond") * 1_000_000 + number("microsecond") * 1_000 + number("nanosecond"),
    )
    .ok_or_else(|| crate::value::error::throw_range_error("Invalid date-time"))?;
    let epoch = chrono::NaiveDateTime::new(date, time)
        .and_utc()
        .timestamp_nanos_opt()
        .ok_or_else(|| crate::value::error::throw_range_error("Invalid date-time"))?;
    Ok(Value::Object(std::rc::Rc::new(
        crate::value::ObjectData::new(vec![
            ("epochNanoseconds".into(), Value::BigInt(epoch.to_string())),
            ("timeZoneId".into(), Value::String(time_zone.clone())),
            ("calendarId".into(), Value::String("iso8601".into())),
            (
                "\0prototype".into(),
                Value::Builtin(crate::ops::Builtin::TemporalZonedDateTimePrototype),
            ),
        ]),
    )))
}

fn object_number(object: &crate::value::ObjectData, name: &str) -> f64 {
    object
        .iter()
        .find(|(key, _)| key == name)
        .and_then(|(_, value)| match value {
            Value::Number(value) => Some(*value),
            _ => None,
        })
        .unwrap_or(0.0)
}

fn getter(builtin: crate::ops::Builtin, receiver: Option<&Value>) -> Result<Value, VmError> {
    let receiver =
        receiver.ok_or_else(|| crate::value::error::throw_type_error("Not a PlainDateTime"))?;
    let name = match builtin {
        crate::ops::Builtin::TemporalPlainDateTimeCalendarIdGetter => "calendarId",
        crate::ops::Builtin::TemporalPlainDateTimeYearGetter => "year",
        crate::ops::Builtin::TemporalPlainDateTimeMonthGetter => "month",
        crate::ops::Builtin::TemporalPlainDateTimeMonthCodeGetter => "monthCode",
        crate::ops::Builtin::TemporalPlainDateTimeDayGetter => "day",
        crate::ops::Builtin::TemporalPlainDateTimeHourGetter => "hour",
        crate::ops::Builtin::TemporalPlainDateTimeMinuteGetter => "minute",
        crate::ops::Builtin::TemporalPlainDateTimeSecondGetter => "second",
        crate::ops::Builtin::TemporalPlainDateTimeMillisecondGetter => "millisecond",
        crate::ops::Builtin::TemporalPlainDateTimeMicrosecondGetter => "microsecond",
        _ => "nanosecond",
    };
    crate::execute::get_property_result(receiver, name)
}

fn fields(value: &Value) -> Result<Vec<f64>, VmError> {
    NAMES
        .iter()
        .map(|name| crate::execute::get_property_result(value, name))
        .map(|value| value.and_then(|value| crate::conversion::to_number(&value)))
        .collect()
}

fn compare(arguments: &[Value]) -> Result<Value, VmError> {
    let left = fields(&from(arguments.first(), None)?)?;
    let right = fields(&from(arguments.get(1), None)?)?;
    Ok(Value::Number(match left.partial_cmp(&right) {
        Some(std::cmp::Ordering::Less) => -1.0,
        Some(std::cmp::Ordering::Greater) => 1.0,
        _ => 0.0,
    }))
}

fn equals(receiver: Option<&Value>, other: Option<&Value>) -> Result<Value, VmError> {
    let receiver =
        receiver.ok_or_else(|| crate::value::error::throw_type_error("Not a PlainDateTime"))?;
    Ok(Value::Boolean(
        fields(receiver)? == fields(&from(other, None)?)?,
    ))
}

fn add(
    receiver: Option<&Value>,
    duration: Option<&Value>,
    direction: f64,
) -> Result<Value, VmError> {
    let receiver =
        receiver.ok_or_else(|| crate::value::error::throw_type_error("Not a PlainDateTime"))?;
    let duration = crate::temporal::duration::from(duration)?;
    let mut values = fields(receiver)?;
    let months = (number_property(&duration, "years") * 12.0
        + number_property(&duration, "months"))
        * direction;
    let total = values[0] * 12.0 + values[1] - 1.0 + months;
    values[0] = (total / 12.0).floor();
    values[1] = total.rem_euclid(12.0) + 1.0;
    let days = (number_property(&duration, "weeks") * 7.0 + number_property(&duration, "days"))
        * direction;
    if days != 0.0 {
        let date = NaiveDate::from_ymd_opt(values[0] as i32, values[1] as u32, values[2] as u32)
            .ok_or_else(|| crate::value::error::throw_range_error("Invalid date-time"))?
            + CalendarDuration::days(days as i64);
        values[0] = date.year() as f64;
        values[1] = date.month() as f64;
        values[2] = date.day() as f64;
    }
    construct(&values.into_iter().map(Value::Number).collect::<Vec<_>>())
}

fn number_property(value: &Value, name: &str) -> f64 {
    crate::execute::get_property_result(value, name)
        .ok()
        .and_then(|value| crate::conversion::to_number(&value).ok())
        .unwrap_or(0.0)
}

fn round(receiver: Option<&Value>, options: Option<&Value>) -> Result<Value, VmError> {
    let receiver =
        receiver.ok_or_else(|| crate::value::error::throw_type_error("Not a PlainDateTime"))?;
    let options = options
        .filter(|value| crate::value::is_object(value))
        .ok_or_else(|| crate::value::error::throw_type_error("Invalid rounding options"))?;
    let unit = crate::execute::get_property_result(options, "smallestUnit")?;
    let Value::String(unit) = unit else {
        return Err(crate::value::error::throw_range_error(
            "Invalid smallestUnit",
        ));
    };
    let quantum = match unit.as_str() {
        "hour" => 3_600_000_000_000.0,
        "minute" => 60_000_000_000.0,
        "second" => 1_000_000_000.0,
        "millisecond" => 1_000_000.0,
        "microsecond" => 1_000.0,
        "nanosecond" => 1.0,
        _ => {
            return Err(crate::value::error::throw_range_error(
                "Invalid smallestUnit",
            ))
        }
    };
    let increment = crate::execute::get_property_result(options, "roundingIncrement")
        .ok()
        .and_then(|value| crate::conversion::to_number(&value).ok())
        .unwrap_or(1.0);
    round_values(fields(receiver)?, quantum, increment)
}

fn round_values(mut values: Vec<f64>, quantum: f64, increment: f64) -> Result<Value, VmError> {
    let total = values[3] * 3_600_000_000_000.0
        + values[4] * 60_000_000_000.0
        + values[5] * 1_000_000_000.0
        + values[6] * 1_000_000.0
        + values[7] * 1_000.0
        + values[8];
    let rounded = (total / (quantum * increment)).round() * quantum * increment;
    values[3] = (rounded / 3_600_000_000_000.0).floor();
    let mut remainder = rounded - values[3] * 3_600_000_000_000.0;
    values[4] = (remainder / 60_000_000_000.0).floor();
    remainder -= values[4] * 60_000_000_000.0;
    values[5] = (remainder / 1_000_000_000.0).floor();
    remainder -= values[5] * 1_000_000_000.0;
    values[6] = (remainder / 1_000_000.0).floor();
    remainder -= values[6] * 1_000_000.0;
    values[7] = (remainder / 1_000.0).floor();
    values[8] = remainder - values[7] * 1_000.0;
    construct(&values.into_iter().map(Value::Number).collect::<Vec<_>>())
}

fn with(
    receiver: Option<&Value>,
    changes: Option<&Value>,
    options: Option<&Value>,
) -> Result<Value, VmError> {
    let receiver =
        receiver.ok_or_else(|| crate::value::error::throw_type_error("Not a PlainDateTime"))?;
    let changes = changes
        .filter(|value| crate::value::is_object(value))
        .ok_or_else(|| crate::value::error::throw_type_error("Invalid date-time"))?;
    let mut values = fields(receiver)?;
    let calendar = crate::execute::get_property_result(changes, "calendar")?;
    if !matches!(calendar, Value::Undefined) {
        return Err(crate::value::error::throw_type_error("Invalid calendar"));
    }
    let month_code = crate::execute::get_property_result(changes, "monthCode")?;
    let month = crate::execute::get_property_result(changes, "month")?;
    if !matches!(month_code, Value::Undefined) {
        values[1] = crate::conversion::to_number(&month_code_number(&month_code)?)?;
    }
    for (index, name) in NAMES.iter().enumerate() {
        let value = crate::execute::get_property_result(changes, name)?;
        if !matches!(value, Value::Undefined) {
            values[index] = if *name == "monthCode" {
                crate::conversion::to_number(&month_code_number(&value)?)?
            } else {
                crate::conversion::to_number(&value)?
            };
        }
    }
    if !matches!(month_code, Value::Undefined)
        && month != Value::Undefined
        && crate::conversion::to_number(&month)?
            != crate::conversion::to_number(&month_code_number(&month_code)?)?
    {
        return Err(crate::value::error::throw_range_error("Month mismatch"));
    }
    let recognized = NAMES.iter().any(|name| {
        crate::execute::get_property_result(changes, name)
            .is_ok_and(|value| !matches!(value, Value::Undefined))
    }) || !matches!(month_code, Value::Undefined);
    if !recognized {
        return Err(crate::value::error::throw_type_error(
            "Insufficient date-time data",
        ));
    }
    let overflow = options
        .and_then(|value| crate::execute::get_property_result(value, "overflow").ok())
        .unwrap_or(Value::String("constrain".into()));
    if values[2] > days_in_month(values[0] as i32, values[1] as u32) as f64 {
        if matches!(overflow, Value::String(value) if value == "constrain") {
            values[2] = days_in_month(values[0] as i32, values[1] as u32) as f64;
        } else {
            return Err(crate::value::error::throw_range_error("Invalid date-time"));
        }
    }
    construct(&values.into_iter().map(Value::Number).collect::<Vec<_>>())
}

fn days_in_month(year: i32, month: u32) -> u32 {
    let (next_year, next_month) = if month == 12 {
        (year + 1, 1)
    } else {
        (year, month + 1)
    };
    chrono::NaiveDate::from_ymd_opt(next_year, next_month, 1)
        .map(|date| (date - chrono::Days::new(1)).day())
        .unwrap_or(28)
}

fn to_string(receiver: Option<&Value>, options: Option<&Value>) -> Result<Value, VmError> {
    let receiver =
        receiver.ok_or_else(|| crate::value::error::throw_type_error("Not a PlainDateTime"))?;
    validate_string_options(options)?;
    let mut values = NAMES
        .iter()
        .map(|name| crate::execute::get_property_result(receiver, name))
        .collect::<Result<Vec<_>, _>>()?
        .into_iter()
        .map(|value| crate::conversion::to_number(&value))
        .collect::<Result<Vec<_>, _>>()?;
    if let Some(Value::String(smallest_unit)) =
        options.and_then(|value| crate::execute::get_property_result(value, "smallestUnit").ok())
    {
        match smallest_unit.strip_suffix('s').unwrap_or(&smallest_unit) {
            "minute" => values[5] = 0.0,
            "second" => {}
            "millisecond" => values[7] = 0.0,
            "microsecond" => values[8] = 0.0,
            _ => {}
        }
        if matches!(
            smallest_unit.as_str(),
            "minute" | "minutes" | "second" | "seconds"
        ) {
            values[6] = 0.0;
            values[7] = 0.0;
            values[8] = 0.0;
        }
    }
    let fraction = values[6] as u32 * 1_000_000 + values[7] as u32 * 1_000 + values[8] as u32;
    let digits = options
        .and_then(|value| crate::execute::get_property_result(value, "fractionalSecondDigits").ok())
        .map(|value| match value {
            Value::Number(value) if (0.0..=9.0).contains(&value) && value.fract() == 0.0 => {
                Ok(value as usize)
            }
            Value::String(value) if value == "auto" => Ok(usize::MAX),
            Value::Undefined => Ok(usize::MAX),
            _ => Err(crate::value::error::throw_range_error(
                "Invalid fractionalSecondDigits",
            )),
        })
        .transpose()?
        .unwrap_or(usize::MAX);
    let suffix = if digits == 0 || (fraction == 0 && digits == usize::MAX) {
        String::new()
    } else {
        let text = format!("{fraction:09}");
        let text = if digits == usize::MAX {
            text.trim_end_matches('0')
        } else {
            &text[..digits]
        };
        format!(".{text}")
    };
    let calendar_suffix = options
        .and_then(|value| crate::execute::get_property_result(value, "calendarName").ok())
        .filter(|value| matches!(value, Value::String(value) if value == "always"))
        .map_or(String::new(), |_| "[u-ca=iso8601]".into());
    let year = year_text(values[0] as i32);
    Ok(Value::String(format!(
        "{year}-{:02}-{:02}T{:02}:{:02}:{:02}{suffix}{calendar_suffix}",
        values[1], values[2], values[3], values[4], values[5]
    )))
}

fn validate_string_options(options: Option<&Value>) -> Result<(), VmError> {
    let Some(options) = options else {
        return Ok(());
    };
    if !crate::value::is_object(options) {
        return Err(crate::value::error::throw_type_error(
            "Invalid string options",
        ));
    }
    if let Value::String(calendar_name) =
        crate::execute::get_property_result(options, "calendarName")?
    {
        if !matches!(calendar_name.as_str(), "auto" | "always" | "never") {
            return Err(crate::value::error::throw_range_error(
                "Invalid calendarName",
            ));
        }
    }
    if let Value::String(smallest_unit) =
        crate::execute::get_property_result(options, "smallestUnit")?
    {
        let unit = smallest_unit.strip_suffix('s').unwrap_or(&smallest_unit);
        if !matches!(
            unit,
            "minute" | "second" | "millisecond" | "microsecond" | "nanosecond"
        ) {
            return Err(crate::value::error::throw_range_error(
                "Invalid smallestUnit",
            ));
        }
    }
    Ok(())
}

fn year_text(year: i32) -> String {
    if year < 0 {
        format!("-{year_abs:06}", year_abs = year.unsigned_abs())
    } else if year > 9999 {
        format!("+{year:06}")
    } else {
        format!("{year:04}")
    }
}

fn from(value: Option<&Value>, options: Option<&Value>) -> Result<Value, VmError> {
    let Some(value) = value else {
        return Err(crate::value::error::throw_type_error("Invalid date-time"));
    };
    if crate::conversion::is_symbol(value) {
        return Err(crate::value::error::throw_type_error("Invalid date-time"));
    }
    if matches!(
        value,
        Value::Builtin(crate::ops::Builtin::TemporalPlainDateTime)
            | Value::Builtin(crate::ops::Builtin::TemporalPlainDateTimePrototype)
    ) {
        return Err(crate::value::error::throw_type_error("Invalid date-time"));
    }
    if let Value::String(text) = value {
        let result = parse_string(text);
        if result.is_ok() {
            validate_options(options)?;
        }
        return result;
    }
    validate_options(options)?;
    if !crate::value::is_object(value) {
        return Err(crate::value::error::throw_type_error("Invalid date-time"));
    }
    if let Some(result) = from_zoned(value, options)? {
        return result;
    }
    if crate::execute::get_property(value, "_temporal_kind") == Value::String("PlainDate".into()) {
        let fields = ["_year", "_month", "_day"]
            .iter()
            .map(|name| crate::execute::get_property(value, name))
            .chain(std::iter::repeat(Value::Number(0.0)).take(6));
        return construct(&fields.collect::<Vec<_>>());
    }
    let year = crate::execute::get_property_result(value, "year")?;
    let day = crate::execute::get_property_result(value, "day")?;
    let month = crate::execute::get_property_result(value, "month")?;
    let month_code = crate::execute::get_property_result(value, "monthCode")?;
    if matches!(year, Value::Undefined)
        || matches!(day, Value::Undefined)
        || (matches!(month, Value::Undefined) && matches!(month_code, Value::Undefined))
    {
        return Err(crate::value::error::throw_type_error(
            "Missing date-time field",
        ));
    }
    let month = if matches!(month, Value::Undefined) {
        month_code_number(&month_code)?
    } else {
        month
    };
    if !matches!(month_code, Value::Undefined)
        && crate::conversion::to_number(&month)?
            != crate::conversion::to_number(&month_code_number(&month_code)?)?
    {
        return Err(crate::value::error::throw_range_error("Month mismatch"));
    }
    let calendar = crate::execute::get_property_result(value, "calendar")?;
    if !matches!(calendar, Value::Undefined) {
        validate_calendar(&calendar)?;
    }
    let mut fields = vec![year, month, day];
    for name in &NAMES[3..] {
        let field = crate::execute::get_property_result(value, name)?;
        fields.push(if matches!(field, Value::Undefined) {
            Value::Number(0.0)
        } else {
            field
        });
    }
    if !overflow_reject(options) {
        constrain_date_fields(&mut fields)?;
    }
    if matches!(fields.get(5), Some(Value::Number(value)) if *value == 60.0)
        && matches!(options.and_then(|value| crate::execute::get_property_result(value, "overflow").ok()), Some(Value::String(value)) if value == "reject")
    {
        return Err(crate::value::error::throw_range_error("Invalid date-time"));
    }
    construct(&fields)
}

fn from_zoned(
    value: &Value,
    _options: Option<&Value>,
) -> Result<Option<Result<Value, VmError>>, VmError> {
    let epoch = crate::execute::get_property_result(value, "epochNanoseconds")?;
    let Value::BigInt(epoch) = epoch else {
        return Ok(None);
    };
    let epoch = epoch
        .parse::<i128>()
        .map_err(|_| crate::value::error::throw_range_error("Invalid date-time"))?;
    let zone = crate::execute::get_property_result(value, "timeZoneId")?;
    let offset = match zone {
        Value::String(zone) if zone.starts_with(['+', '-']) => parse_offset_minutes(&zone)?,
        _ => 0,
    };
    let local = epoch + i128::from(offset) * 60_000_000_000;
    let seconds = local.div_euclid(1_000_000_000) as i64;
    let nanos = local.rem_euclid(1_000_000_000) as u32;
    let date_time = chrono::DateTime::from_timestamp(seconds, nanos)
        .ok_or_else(|| crate::value::error::throw_range_error("Invalid date-time"))?;
    let fields = vec![
        Value::Number(date_time.year() as f64),
        Value::Number(date_time.month() as f64),
        Value::Number(date_time.day() as f64),
        Value::Number(date_time.hour() as f64),
        Value::Number(date_time.minute() as f64),
        Value::Number(date_time.second() as f64),
        Value::Number((nanos / 1_000_000) as f64),
        Value::Number(((nanos / 1_000) % 1_000) as f64),
        Value::Number((nanos % 1_000) as f64),
    ];
    Ok(Some(construct(&fields)))
}

fn parse_offset_minutes(zone: &str) -> Result<i64, VmError> {
    let sign = if zone.starts_with('-') { -1 } else { 1 };
    let parts = zone[1..].split(':').collect::<Vec<_>>();
    if parts.len() != 2 {
        return Err(crate::value::error::throw_range_error("Invalid time zone"));
    }
    let hours = parts[0]
        .parse::<i64>()
        .map_err(|_| crate::value::error::throw_range_error("Invalid time zone"))?;
    let minutes = parts[1]
        .parse::<i64>()
        .map_err(|_| crate::value::error::throw_range_error("Invalid time zone"))?;
    Ok(sign * (hours * 60 + minutes))
}

fn overflow_reject(options: Option<&Value>) -> bool {
    options
        .and_then(|value| crate::execute::get_property_result(value, "overflow").ok())
        .is_some_and(|value| matches!(value, Value::String(value) if value == "reject"))
}

fn constrain_date_fields(fields: &mut [Value]) -> Result<(), VmError> {
    let year = crate::conversion::to_number(&fields[0])?;
    let month = crate::conversion::to_number(&fields[1])?;
    let day = crate::conversion::to_number(&fields[2])?;
    if month <= 0.0 || day <= 0.0 {
        return Err(crate::value::error::throw_range_error("Invalid date-time"));
    }
    let month = month.clamp(1.0, 12.0);
    fields[1] = Value::Number(month);
    let max_day = days_in_month(year as i32, month as u32);
    fields[2] = Value::Number(day.clamp(1.0, max_day as f64));
    Ok(())
}

fn validate_options(options: Option<&Value>) -> Result<(), VmError> {
    let Some(options) = options.filter(|value| !matches!(value, Value::Undefined)) else {
        return Ok(());
    };
    if !crate::value::is_object(options) {
        return Err(crate::value::error::throw_type_error("Invalid options"));
    }
    let overflow = crate::execute::get_property_result(options, "overflow")?;
    if matches!(overflow, Value::Undefined) {
        return Ok(());
    }
    if crate::conversion::is_symbol(&overflow) {
        return Err(crate::value::error::throw_type_error("Invalid overflow"));
    }
    let overflow = crate::conversion::to_string(&overflow)?;
    if overflow == "constrain" || overflow == "reject" {
        Ok(())
    } else {
        Err(crate::value::error::throw_range_error("Invalid overflow"))
    }
}

fn month_code_number(value: &Value) -> Result<Value, VmError> {
    let Value::String(code) = value else {
        return Err(crate::value::error::throw_range_error("Invalid monthCode"));
    };
    code.strip_prefix('M')
        .and_then(|value| value.parse::<f64>().ok())
        .map(Value::Number)
        .ok_or_else(|| crate::value::error::throw_range_error("Invalid monthCode"))
}

fn validate_calendar(value: &Value) -> Result<(), VmError> {
    if crate::conversion::is_symbol(value) {
        return Err(crate::value::error::throw_type_error("Invalid calendar"));
    }
    if crate::value::is_object(value) {
        return Ok(());
    }
    let Value::String(calendar) = value else {
        return Err(crate::value::error::throw_type_error("Invalid calendar"));
    };
    let iso_string = calendar
        .chars()
        .next()
        .is_some_and(|character| character.is_ascii_digit())
        && calendar.contains('-')
        && (!calendar.contains('[') || calendar.contains("[u-ca=iso8601]"));
    if calendar.eq_ignore_ascii_case("iso8601") || iso_string {
        Ok(())
    } else {
        Err(crate::value::error::throw_range_error("Invalid calendar"))
    }
}

fn parse_string(text: &str) -> Result<Value, VmError> {
    validate_annotations(text)?;
    let mut calendar_checked = false;
    for annotation in text.split('[').skip(1) {
        let Some(calendar) = annotation
            .trim_end_matches(']')
            .strip_prefix("u-ca=")
            .or_else(|| annotation.trim_end_matches(']').strip_prefix("!u-ca="))
        else {
            continue;
        };
        if calendar.len() >= 10
            && calendar.as_bytes().get(4) == Some(&b'-')
            && calendar.as_bytes().get(7) == Some(&b'-')
        {
            return Err(crate::value::error::throw_range_error("Invalid calendar"));
        }
        if !calendar_checked {
            validate_calendar(&Value::String(calendar.to_string()))?;
            calendar_checked = true;
        }
    }
    if text.split('[').skip(1).any(|part| {
        part.split_once('=')
            .is_some_and(|(key, _)| key.chars().any(|character| character.is_ascii_uppercase()))
    }) {
        return Err(crate::value::error::throw_range_error("Invalid annotation"));
    }
    let main = text.split('[').next().unwrap_or(text);
    let (date, time) = main
        .split_once('T')
        .or_else(|| main.split_once('t'))
        .or_else(|| main.split_once(' '))
        .unwrap_or((main, "00:00"));
    validate_time_offset(time)?;
    validate_fraction_digits(time)?;
    let time = time.split(['+', '-']).next().unwrap_or(time);
    if date.starts_with("-000000") {
        return Err(crate::value::error::throw_range_error("Invalid date-time"));
    }
    if !date.starts_with(['+', '-']) && date.contains('-') && {
        let parts = date.split('-').collect::<Vec<_>>();
        parts.len() != 3 || parts[0].len() != 4 || parts[1].len() != 2 || parts[2].len() != 2
    } {
        return Err(crate::value::error::throw_range_error("Invalid date-time"));
    }
    let date_fields = if date.starts_with(['+', '-']) && !date.contains('-') && date.len() == 11 {
        vec![&date[..7], &date[7..9], &date[9..]]
    } else if date.starts_with(['+', '-']) && date.matches('-').count() >= 2 {
        vec![&date[..7], &date[8..10], &date[11..]]
    } else if date.contains('-') {
        date.split('-').collect::<Vec<_>>()
    } else if date.len() == 8 {
        vec![&date[..4], &date[4..6], &date[6..]]
    } else {
        return Err(crate::value::error::throw_range_error("Invalid date-time"));
    };
    if date_fields.len() != 3 {
        return Err(crate::value::error::throw_range_error("Invalid date-time"));
    }
    let (clock, fraction) = time
        .split_once('.')
        .or_else(|| time.split_once(','))
        .map_or((time, ""), |parts| parts);
    let clock = if !clock.contains(':') && clock.len() == 2 {
        format!("{clock}:00")
    } else if !clock.contains(':') && clock.len() == 4 {
        format!("{}:{}", &clock[..2], &clock[2..])
    } else if !clock.contains(':') && clock.len() == 6 {
        format!("{}:{}:{}", &clock[..2], &clock[2..4], &clock[4..])
    } else {
        clock.to_string()
    };
    if clock.contains(':') && clock.split(':').any(|part| part.len() != 2) {
        return Err(crate::value::error::throw_range_error("Invalid date-time"));
    }
    let clock = clock.split(':').collect::<Vec<_>>();
    if clock.len() < 2 || clock.len() > 3 || fraction.len() > 9 {
        return Err(crate::value::error::throw_range_error("Invalid date-time"));
    }
    let mut fields = date_fields
        .into_iter()
        .chain(clock)
        .map(|part| part.parse::<f64>().unwrap_or(f64::NAN))
        .collect::<Vec<_>>();
    if fields.get(5) == Some(&60.0) {
        fields[5] = 59.0;
    }
    let nanos = format!("{fraction:0<9}").parse::<f64>().unwrap_or(0.0);
    fields.extend([
        (nanos / 1_000_000.0).trunc(),
        (nanos / 1_000.0).trunc() % 1_000.0,
        nanos % 1_000.0,
    ]);
    construct(&fields.into_iter().map(Value::Number).collect::<Vec<_>>())
}

fn validate_fraction_digits(time: &str) -> Result<(), VmError> {
    for separator in ['.', ','] {
        if let Some(index) = time.find(separator) {
            let digits = time[index + 1..]
                .chars()
                .take_while(|character| character.is_ascii_digit())
                .count();
            if digits > 9 {
                return Err(crate::value::error::throw_range_error("Invalid fraction"));
            }
        }
    }
    Ok(())
}

fn validate_time_offset(time: &str) -> Result<(), VmError> {
    let Some(index) = time
        .get(1..)
        .and_then(|tail| tail.find(['+', '-']).map(|index| index + 1))
    else {
        return Ok(());
    };
    let offset = &time[index + 1..];
    let offset = offset
        .split_once(['.', ','])
        .map_or(offset, |(prefix, _)| prefix);
    let valid = offset
        .chars()
        .all(|character| character.is_ascii_digit() || character == ':')
        && matches!(offset.len(), 2 | 4 | 5 | 6 | 8);
    if valid {
        Ok(())
    } else {
        Err(crate::value::error::throw_range_error("Invalid offset"))
    }
}

fn validate_annotations(text: &str) -> Result<(), VmError> {
    let mut calendars = 0;
    let mut critical_calendar = false;
    let mut time_zones = 0;
    for annotation in text.split('[').skip(1) {
        if let Some(close) = annotation.find(']') {
            let trailing = &annotation[close + 1..];
            if !trailing.is_empty() && !trailing.starts_with('[') {
                return Err(crate::value::error::throw_range_error("Invalid annotation"));
            }
        }
        let annotation = annotation
            .strip_suffix(']')
            .unwrap_or(annotation)
            .split('[')
            .next()
            .unwrap_or(annotation);
        if annotation.starts_with('!')
            && annotation.contains('=')
            && !annotation.starts_with("!u-ca=")
        {
            return Err(crate::value::error::throw_range_error("Invalid annotation"));
        }
        if annotation.starts_with("u-ca=") || annotation.starts_with("!u-ca=") {
            calendars += 1;
            critical_calendar |= annotation.starts_with("!u-ca=");
        } else if !annotation.contains('=') {
            time_zones += 1;
        }
    }
    if (calendars > 1 && critical_calendar) || time_zones > 1 {
        return Err(crate::value::error::throw_range_error(
            "Duplicate annotation",
        ));
    }
    Ok(())
}
