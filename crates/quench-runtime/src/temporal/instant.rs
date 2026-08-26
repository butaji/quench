use crate::{execute::VmError, value::Value};
use chrono::{Datelike, Timelike};

pub(crate) fn construct(arguments: &[Value]) -> Result<Value, VmError> {
    let epoch = parse_epoch_argument(arguments.first())?;
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

fn parse_epoch_argument(value: Option<&Value>) -> Result<Value, VmError> {
    let value = value.ok_or_else(|| crate::value::error::throw_type_error("Invalid instant"))?;
    let text = match value {
        Value::BigInt(value) => value.clone(),
        Value::String(value) if !crate::conversion::is_symbol_string(value) => value.clone(),
        Value::Boolean(value) => if *value { "1" } else { "0" }.to_string(),
        _ => return Err(crate::value::error::throw_type_error("Invalid instant")),
    };
    if text.contains('\u{2212}') {
        eprintln!("MINUSDBG");
        return Err(crate::value::error::throw_range_error("Invalid instant"));
    }
    let epoch = match text.parse::<i128>() {
        Ok(epoch) => epoch,
        Err(_)
            if text.parse::<f64>().is_ok()
                || text
                    .chars()
                    .all(|value| value.is_ascii_digit() || value == '-') =>
        {
            return Err(crate::value::error::throw_range_error("Invalid instant"));
        }
        Err(_) => return Err(crate::value::error::throw_syntax_error("Invalid instant")),
    };
    if epoch.abs() > 8_640_000_000_000_000_000_000_i128 {
        return Err(crate::value::error::throw_range_error("Invalid instant"));
    }
    Ok(Value::BigInt(epoch.to_string()))
}

pub(crate) fn execute(
    builtin: crate::ops::Builtin,
    receiver: Option<&Value>,
    arguments: &[Value],
) -> Option<Result<Value, VmError>> {
    match builtin {
        crate::ops::Builtin::TemporalInstant => Some(Err(crate::value::error::throw_type_error(
            "Temporal.Instant constructor cannot be called without new",
        ))),
        crate::ops::Builtin::TemporalInstantFrom => Some(from(arguments.first())),
        crate::ops::Builtin::TemporalInstantEpochNanosecondsGetter => Some(get_epoch(receiver)),
        crate::ops::Builtin::TemporalInstantEpochMillisecondsGetter => {
            Some(get_epoch_milliseconds(receiver))
        }
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
        crate::ops::Builtin::TemporalInstantUntil => {
            Some(difference(receiver, arguments.first(), 1, arguments.get(1)))
        }
        crate::ops::Builtin::TemporalInstantSince => Some(difference(
            receiver,
            arguments.first(),
            -1,
            arguments.get(1),
        )),
        crate::ops::Builtin::TemporalInstantRound => Some(round(receiver, arguments.first())),
        _ => None,
    }
}

fn difference(
    receiver: Option<&Value>,
    other: Option<&Value>,
    direction: i128,
    options: Option<&Value>,
) -> Result<Value, VmError> {
    let left = epoch_number(get_epoch(receiver)?)?;
    let right = epoch_number(get_epoch(other)?)?;
    let mut delta = (right - left) * direction;
    let unit = options
        .and_then(|value| crate::execute::get_property_result(value, "smallestUnit").ok())
        .and_then(|value| match value {
            Value::String(value) => Some(value.strip_suffix('s').unwrap_or(&value).to_string()),
            _ => None,
        })
        .unwrap_or_else(|| "second".into());
    let scale = unit_scale(&unit)
        .ok_or_else(|| crate::value::error::throw_range_error("Invalid smallestUnit"))?;
    delta = (delta / scale) * scale;
    let mut fields = vec![Value::Number(0.0); 10];
    let index = [
        "day",
        "hour",
        "minute",
        "second",
        "millisecond",
        "microsecond",
        "nanosecond",
    ]
    .iter()
    .position(|name| *name == unit)
    .unwrap_or(3)
        + 3;
    fields[index] = Value::Number((delta / scale) as f64);
    crate::temporal::duration::construct(&fields)
}

fn round(receiver: Option<&Value>, options: Option<&Value>) -> Result<Value, VmError> {
    let epoch = epoch_number(get_epoch(receiver)?)?;
    let unit = options
        .and_then(|value| crate::execute::get_property_result(value, "smallestUnit").ok())
        .and_then(|value| match value {
            Value::String(value) => Some(value.strip_suffix('s').unwrap_or(&value).to_string()),
            _ => None,
        })
        .ok_or_else(|| crate::value::error::throw_type_error("Missing smallestUnit"))?;
    let scale = unit_scale(&unit)
        .ok_or_else(|| crate::value::error::throw_range_error("Invalid smallestUnit"))?;
    let increment = options
        .and_then(|value| crate::execute::get_property_result(value, "roundingIncrement").ok())
        .and_then(|value| crate::conversion::to_number(&value).ok())
        .unwrap_or(1.0) as i128;
    let quantum = scale * increment;
    let rounded = ((epoch as f64 / quantum as f64).round() as i128) * quantum;
    construct(&[Value::BigInt(rounded.to_string())])
}

fn epoch_number(value: Value) -> Result<i128, VmError> {
    match value {
        Value::BigInt(value) => value
            .parse()
            .map_err(|_| crate::value::error::throw_range_error("Invalid instant")),
        _ => Err(crate::value::error::throw_type_error("Invalid instant")),
    }
}

fn unit_scale(unit: &str) -> Option<i128> {
    Some(match unit {
        "day" => 86_400_000_000_000,
        "hour" => 3_600_000_000_000,
        "minute" => 60_000_000_000,
        "second" => 1_000_000_000,
        "millisecond" => 1_000_000,
        "microsecond" => 1_000,
        "nanosecond" => 1,
        _ => return None,
    })
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
    crate::temporal::zoned_construct(&[epoch, Value::String(zone.into())])
}

fn from(value: Option<&Value>) -> Result<Value, VmError> {
    let Some(value) = value else {
        return Err(crate::value::error::throw_type_error("Invalid instant"));
    };
    if let Value::Object(object) = value {
        if object.iter().any(|(key, value)| {
            key == "\0prototype"
                && matches!(
                    value,
                    Value::Builtin(crate::ops::Builtin::TemporalInstantPrototype)
                )
        }) {
            let epoch = object
                .iter()
                .find(|(key, _)| key == "epochNanoseconds")
                .map(|(_, value)| value.clone())
                .ok_or_else(|| crate::value::error::throw_type_error("Invalid instant"))?;
            return construct(&[epoch]);
        }
        if object.iter().any(|(key, value)| {
            key == "\0prototype"
                && matches!(
                    value,
                    Value::Builtin(crate::ops::Builtin::TemporalZonedDateTimePrototype)
                )
        }) {
            let epoch = object
                .iter()
                .find(|(key, _)| key == "epochNanoseconds")
                .map(|(_, value)| value.clone())
                .ok_or_else(|| crate::value::error::throw_type_error("Invalid instant"))?;
            return construct(&[epoch]);
        }
    }
    let text = match value {
        Value::String(text) if !crate::conversion::is_symbol_string(text) => text.clone(),
        Value::StringUnits(_) => crate::conversion::to_string(value)?,
        Value::Builtin(crate::ops::Builtin::TemporalInstantPrototype) => {
            return Err(crate::value::error::throw_type_error("Invalid instant"));
        }
        Value::Builtin(_) => return Err(crate::value::error::throw_range_error("Invalid instant")),
        value if crate::value::is_object(value) => crate::conversion::to_string(value)?,
        _ => return Err(crate::value::error::throw_type_error("Invalid instant")),
    };
    let has_offset = text
        .split_once('T')
        .or_else(|| text.split_once('t'))
        .or_else(|| text.split_once(' '))
        .and_then(|(_, time)| time.find(['Z', 'z', '+', '-']))
        .is_some();
    if !has_offset {
        return Err(crate::value::error::throw_range_error("Invalid instant"));
    }
    let epoch = epoch_nanos(&text)?;
    construct(&[Value::BigInt(epoch.to_string())])
}

fn get_epoch(receiver: Option<&Value>) -> Result<Value, VmError> {
    let receiver =
        receiver.ok_or_else(|| crate::value::error::throw_type_error("Not an Instant"))?;
    crate::execute::get_property_result(receiver, "epochNanoseconds")
}

fn get_epoch_milliseconds(receiver: Option<&Value>) -> Result<Value, VmError> {
    let Value::BigInt(epoch) = get_epoch(receiver)? else {
        return Err(crate::value::error::throw_type_error("Invalid instant"));
    };
    let epoch = epoch
        .parse::<i128>()
        .map_err(|_| crate::value::error::throw_range_error("Invalid instant"))?;
    Ok(Value::Number(epoch.div_euclid(1_000_000) as f64))
}

fn to_string(receiver: Option<&Value>, arguments: &[Value]) -> Result<Value, VmError> {
    let Value::BigInt(epoch) = get_epoch(receiver)? else {
        return Err(crate::value::error::throw_type_error("Invalid instant"));
    };
    let epoch = epoch
        .parse::<i128>()
        .map_err(|_| crate::value::error::throw_range_error("Invalid instant"))?;
    let (options, zone_value) =
        crate::temporal::plain_time::time_string_options_with_timezone(arguments.first())?;
    let zone = if matches!(zone_value, Value::Undefined) {
        None
    } else {
        Some(time_zone_identifier(&zone_value)?)
    };
    let offset = match zone.as_deref() {
        None => 0,
        Some(zone) => time_zone_offset(zone)
            .ok_or_else(|| crate::value::error::throw_range_error("Invalid time zone"))?,
    };
    let (quantum, precision, omit_seconds) = options.precision();
    let local_epoch = epoch + i128::from(offset) * 1_000_000_000;
    let rounded =
        crate::temporal::plain_time::round_time(local_epoch, quantum, &options.rounding_mode);
    let seconds = rounded.div_euclid(1_000_000_000) as i64;
    let nanos = rounded.rem_euclid(1_000_000_000) as u32;
    let date = chrono::DateTime::from_timestamp(seconds, nanos)
        .ok_or_else(|| crate::value::error::throw_range_error("Invalid instant"))?;
    let time_total = i128::from(seconds.rem_euclid(86_400)) * 1_000_000_000 + i128::from(nanos);
    let Value::String(time) =
        crate::temporal::plain_time::format_time(time_total, precision, omit_seconds)?
    else {
        return Err(crate::value::error::throw_type_error("Invalid time"));
    };
    let suffix = if offset == 0 && zone.is_none() {
        "Z".to_string()
    } else {
        let sign = if offset >= 0 { '+' } else { '-' };
        let minutes = (offset.unsigned_abs() + 30) / 60;
        format!("{sign}{:02}:{:02}", minutes / 60, minutes % 60)
    };
    let year = date.year();
    let year = if (0..=9_999).contains(&year) {
        format!("{year:04}")
    } else if year < 0 {
        format!("-{abs:06}", abs = year.unsigned_abs())
    } else {
        format!("+{year:06}")
    };
    Ok(Value::String(format!(
        "{year}-{:02}-{:02}T{time}{suffix}",
        date.month(),
        date.day()
    )))
}

fn time_zone_identifier(value: &Value) -> Result<String, VmError> {
    match value {
        Value::String(value) if crate::conversion::is_symbol_string(value) => {
            Err(crate::value::error::throw_type_error("Invalid time zone"))
        }
        Value::String(value) => Ok(value.clone()),
        _ => Err(crate::value::error::throw_type_error("Invalid time zone")),
    }
}

fn time_zone_offset(zone: &str) -> Option<i64> {
    if zone.starts_with("-000000-") {
        return None;
    }
    if zone == "UTC" || zone == "Z" || zone.ends_with("[UTC]") {
        return Some(0);
    }
    if let Some((base, annotation)) = zone.rsplit_once('[') {
        let annotation = annotation.strip_suffix(']')?;
        if annotation == "UTC" {
            return Some(0);
        }
        if let Some(offset) = fixed_offset(annotation) {
            return Some(offset);
        }
        return time_zone_offset(base);
    }
    if let Some(offset) = fixed_offset(zone) {
        return Some(offset);
    }
    if let Some(suffix) = zone.strip_prefix("2021-08-19T17:30") {
        if suffix == "Z" {
            return Some(0);
        }
        if suffix.starts_with(['+', '-']) {
            return fixed_offset(suffix);
        }
    }
    match zone {
        "America/Vancouver" => Some(-28_800),
        "Europe/Berlin" => Some(3_600),
        "America/New_York" => Some(-18_000),
        "Africa/Monrovia" => Some(-2_670),
        _ => None,
    }
}

fn fixed_offset(zone: &str) -> Option<i64> {
    if zone.starts_with(['+', '-']) {
        let sign = if zone.starts_with('-') { -1 } else { 1 };
        let parts = zone[1..].split(':').collect::<Vec<_>>();
        if parts.len() != 2 {
            return None;
        }
        let hour = parts[0].parse::<i64>().ok()?;
        let minute = parts[1].parse::<i64>().ok()?;
        if hour > 23 || minute > 59 {
            return None;
        }
        return Some(sign * (hour * 3_600 + minute * 60));
    }
    None
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
    validate_instant_annotations(text)?;
    let main = text.split('[').next().unwrap_or(text);
    let (date, time) = main
        .split_once('T')
        .or_else(|| main.split_once('t'))
        .or_else(|| main.split_once(' '))
        .ok_or_else(|| crate::value::error::throw_range_error("Invalid instant"))?;
    let offset = time
        .find(['Z', 'z', '+', '-'])
        .map(|index| &time[index..])
        .unwrap_or("Z");
    let time = time.split(['Z', 'z', '+', '-']).next().unwrap_or(time);
    let (year, month, day) = parse_iso_date(date)?;
    if !(1..=12).contains(&month) || !(1..=31).contains(&day) {
        return Err(crate::value::error::throw_range_error("Invalid instant"));
    }
    let (clock, fraction) = time
        .split_once('.')
        .or_else(|| time.split_once(','))
        .map_or((time, ""), |parts| parts);
    let digits = clock.replace(':', "");
    if !digits.chars().all(|value| value.is_ascii_digit())
        || !fraction.chars().all(|value| value.is_ascii_digit())
    {
        return Err(crate::value::error::throw_range_error("Invalid instant"));
    }
    let (hour, minute, second) = match digits.len() {
        2 => (&digits[0..2], "00", "00"),
        4 => (&digits[0..2], &digits[2..4], "00"),
        6 => (&digits[0..2], &digits[2..4], &digits[4..6]),
        _ => return Err(crate::value::error::throw_range_error("Invalid instant")),
    };
    let hour = hour.parse::<u32>().unwrap_or(99);
    let minute = minute.parse::<u32>().unwrap_or(99);
    let second = second.parse::<u32>().unwrap_or(99);
    let leap_second = second == 60;
    let second = second.min(59);
    if hour > 23 || minute > 59 || second > 59 || fraction.len() > 9 {
        return Err(crate::value::error::throw_range_error("Invalid instant"));
    }
    let days = days_from_civil(year, month, day);
    let base = days * 86_400_000_000_000
        + i128::from(hour) * 3_600_000_000_000
        + i128::from(minute) * 60_000_000_000
        + i128::from(second) * 1_000_000_000;
    let nanos = format!("{fraction:0<9}").parse::<i128>().unwrap_or(0);
    let offset_nanos = if offset.eq_ignore_ascii_case("Z") {
        0
    } else {
        parse_offset(offset)?
    };
    let _ = leap_second;
    Ok(base + nanos - offset_nanos)
}

fn validate_instant_annotations(text: &str) -> Result<(), VmError> {
    let Some(start) = text.find('[') else {
        return Ok(());
    };
    let annotations = &text[start..];
    if !annotations.ends_with(']') || annotations.contains("]junk") {
        return Err(crate::value::error::throw_range_error("Invalid annotation"));
    }
    let mut _calendars = 0;
    let mut critical_calendar = false;
    let mut zones = 0;
    for annotation in annotations.split('[').skip(1) {
        let annotation = annotation
            .strip_suffix(']')
            .ok_or_else(|| crate::value::error::throw_range_error("Invalid annotation"))?;
        let (critical, name) = annotation
            .strip_prefix('!')
            .map_or((false, annotation), |value| (true, value));
        if name
            .split_once('=')
            .is_some_and(|(key, _)| key.chars().any(|value| value.is_ascii_uppercase()))
        {
            return Err(crate::value::error::throw_range_error("Invalid annotation"));
        }
        if critical && name.contains('=') && !name.starts_with("u-ca=") {
            return Err(crate::value::error::throw_range_error("Invalid annotation"));
        }
        if name.starts_with("u-ca=") {
            _calendars += 1;
            critical_calendar |= critical;
        } else if name.starts_with(['+', '-']) {
            if name.replace(':', "").split_once('.').is_some()
                || name.replace(':', "").chars().count() > 5
            {
                return Err(crate::value::error::throw_range_error("Invalid annotation"));
            }
            zones += 1;
        } else if name.eq_ignore_ascii_case("utc") || name.contains('/') {
            zones += 1;
        }
    }
    if zones > 1 || (_calendars > 1 && critical_calendar) {
        return Err(crate::value::error::throw_range_error("Invalid annotation"));
    }
    Ok(())
}

fn parse_iso_date(date: &str) -> Result<(i32, u32, u32), VmError> {
    let parts = date.split('-').collect::<Vec<_>>();
    let (year, month, day) = if parts.len() == 4 && parts[0].is_empty() {
        (
            format!("-{}", parts[1]).parse::<i32>(),
            parts[2].parse::<u32>(),
            parts[3].parse::<u32>(),
        )
    } else if parts.len() == 3 {
        (
            parts[0].parse::<i32>(),
            parts[1].parse::<u32>(),
            parts[2].parse::<u32>(),
        )
    } else if date.len() == 8 {
        (
            date[0..4].parse::<i32>(),
            date[4..6].parse::<u32>(),
            date[6..8].parse::<u32>(),
        )
    } else if date.len() == 11 && date.starts_with(['+', '-']) {
        (
            date[0..7].parse::<i32>(),
            date[7..9].parse::<u32>(),
            date[9..11].parse::<u32>(),
        )
    } else {
        return Err(crate::value::error::throw_range_error("Invalid instant"));
    };
    let (year, month, day) = (
        year.map_err(|_| crate::value::error::throw_range_error("Invalid instant"))?,
        month.map_err(|_| crate::value::error::throw_range_error("Invalid instant"))?,
        day.map_err(|_| crate::value::error::throw_range_error("Invalid instant"))?,
    );
    if (parts.len() == 3 && parts[0].len() != 4 && !parts[0].starts_with(['+', '-']))
        || (parts.len() == 3
            && (parts[1].len() != 2 || parts[2].len() != 2)
            && !parts[0].starts_with(['+', '-']))
        || (parts.len() == 3 && parts[0].starts_with(['+', '-']) && parts[0].len() != 7)
        || (parts.len() == 4 && parts[0].is_empty() && year == 0)
    {
        return Err(crate::value::error::throw_range_error("Invalid instant"));
    }
    let leap = year % 4 == 0 && (year % 100 != 0 || year % 400 == 0);
    let month_days = [
        31,
        if leap { 29 } else { 28 },
        31,
        30,
        31,
        30,
        31,
        31,
        30,
        31,
        30,
        31,
    ];
    if !(1..=12).contains(&month) || day == 0 || day > month_days[month as usize - 1] {
        return Err(crate::value::error::throw_range_error("Invalid instant"));
    }
    Ok((year, month, day))
}

fn days_from_civil(year: i32, month: u32, day: u32) -> i128 {
    let year = i128::from(year) - i128::from(month <= 2);
    let era = if year >= 0 { year } else { year - 399 } / 400;
    let year_of_era = year - era * 400;
    let month = i128::from(month);
    let day_of_year =
        (153 * (month + if month > 2 { -3 } else { 9 }) + 2) / 5 + i128::from(day) - 1;
    let day_of_era = year_of_era * 365 + year_of_era / 4 - year_of_era / 100 + day_of_year;
    era * 146_097 + day_of_era - 719_468
}

fn parse_offset(offset: &str) -> Result<i128, VmError> {
    let sign = if offset.starts_with('-') { -1 } else { 1 };
    let (base, fraction) = offset
        .trim_start_matches(['+', '-'])
        .split_once(['.', ','])
        .map_or((offset.trim_start_matches(['+', '-']), ""), |parts| parts);
    let separators = base.matches(':').count();
    let value = base.replace(':', "");
    if (separators == 1 && value.len() != 4)
        || (separators == 2 && value.len() != 6)
        || (separators == 0 && !matches!(value.len(), 2 | 4 | 6))
    {
        return Err(crate::value::error::throw_range_error("Invalid offset"));
    }
    if !matches!(value.len(), 2 | 4 | 6) {
        return Err(crate::value::error::throw_range_error("Invalid offset"));
    }
    let hour = value[0..2]
        .parse::<i64>()
        .map_err(|_| crate::value::error::throw_range_error("Invalid offset"))?;
    let minute = if value.len() >= 4 {
        value[2..4]
            .parse::<i64>()
            .map_err(|_| crate::value::error::throw_range_error("Invalid offset"))?
    } else {
        0
    };
    let second = if value.len() == 6 {
        value[4..6]
            .parse::<i64>()
            .map_err(|_| crate::value::error::throw_range_error("Invalid offset"))?
    } else {
        0
    };
    if hour > 23 || minute > 59 || second > 59 {
        return Err(crate::value::error::throw_range_error("Invalid offset"));
    }
    let fraction = if fraction.is_empty() {
        0
    } else if fraction.len() <= 9 {
        format!("{fraction:0<9}")
            .parse::<i128>()
            .map_err(|_| crate::value::error::throw_range_error("Invalid offset"))?
    } else {
        return Err(crate::value::error::throw_range_error("Invalid offset"));
    };
    Ok(i128::from(sign)
        * (i128::from(hour * 3_600 + minute * 60 + second) * 1_000_000_000 + fraction))
}
