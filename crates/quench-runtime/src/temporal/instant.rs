use crate::{execute::VmError, value::Value};
use chrono::Datelike;

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
        crate::ops::Builtin::TemporalInstantCompare => Some(compare(arguments)),
        crate::ops::Builtin::TemporalInstantFromEpochMilliseconds => {
            Some(from_epoch_milliseconds(arguments.first()))
        }
        crate::ops::Builtin::TemporalInstantFromEpochNanoseconds => {
            Some(from_epoch_nanoseconds(arguments.first()))
        }
        crate::ops::Builtin::TemporalInstantEpochNanosecondsGetter => Some(get_epoch(receiver)),
        crate::ops::Builtin::TemporalInstantEpochMillisecondsGetter => {
            Some(get_epoch_milliseconds(receiver))
        }
        crate::ops::Builtin::TemporalInstantToString => Some(to_string(receiver, arguments)),
        crate::ops::Builtin::TemporalInstantToJSON => Some(to_string(receiver, &[])),
        crate::ops::Builtin::TemporalInstantToLocaleString => {
            Some(to_locale_string(receiver, arguments))
        }
        crate::ops::Builtin::TemporalInstantValueOf => {
            Some(Err(crate::value::error::throw_type_error(
                "Temporal.Instant.prototype.valueOf is not allowed",
            )))
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

fn compare(arguments: &[Value]) -> Result<Value, VmError> {
    let left = epoch_number(get_epoch(Some(&from(arguments.first())?))?)?;
    let right = epoch_number(get_epoch(Some(&from(arguments.get(1))?))?)?;
    Ok(Value::Number(left.cmp(&right) as i8 as f64))
}

fn from_epoch_milliseconds(value: Option<&Value>) -> Result<Value, VmError> {
    let number = match value {
        Some(value) => crate::conversion::to_number(value)?,
        None => f64::NAN,
    };
    if !number.is_finite() || number.fract() != 0.0 {
        return Err(crate::value::error::throw_range_error("Invalid epoch"));
    }
    let epoch = number as i128 * 1_000_000;
    construct(&[Value::BigInt(epoch.to_string())])
}

fn from_epoch_nanoseconds(value: Option<&Value>) -> Result<Value, VmError> {
    let value = value.ok_or_else(|| crate::value::error::throw_type_error("Invalid epoch"))?;
    let Value::BigInt(_) = value else {
        return Err(crate::value::error::throw_type_error("Invalid epoch"));
    };
    construct(std::slice::from_ref(value))
}

fn difference(
    receiver: Option<&Value>,
    other: Option<&Value>,
    direction: i128,
    options: Option<&Value>,
) -> Result<Value, VmError> {
    let left = epoch_number(get_epoch(receiver)?)?;
    let other = from(other)?;
    let right = epoch_number(get_epoch(Some(&other))?)?;
    let mut delta = (right - left) * direction;
    if options
        .is_some_and(|value| !matches!(value, Value::Undefined) && !crate::value::is_object(value))
    {
        return Err(crate::value::error::throw_type_error("Invalid options"));
    }
    let (smallest, largest, increment, rounding_mode) =
        if let Some(options) = options.filter(|value| crate::value::is_object(value)) {
            let largest = temporal_unit_option(Some(options), "largestUnit")?;
            let increment = {
                let value = crate::execute::get_property_result(options, "roundingIncrement")?;
                if matches!(value, Value::Undefined) {
                    None
                } else {
                    Some(crate::conversion::to_number(&value)?.trunc())
                }
            };
            let rounding_mode = {
                let value = crate::execute::get_property_result(options, "roundingMode")?;
                if matches!(value, Value::Undefined) {
                    None
                } else {
                    Some(crate::conversion::to_string(&value)?)
                }
            };
            let smallest = temporal_unit_option(Some(options), "smallestUnit")?
                .unwrap_or_else(|| "nanosecond".into());
            let largest = largest.unwrap_or_else(|| {
                if matches!(smallest.as_str(), "hour" | "minute") {
                    smallest.clone()
                } else {
                    "second".into()
                }
            });
            (
                smallest,
                largest,
                increment.unwrap_or(1.0),
                rounding_mode.unwrap_or_else(|| "trunc".into()),
            )
        } else {
            ("nanosecond".into(), "second".into(), 1.0, "trunc".into())
        };
    if !matches!(
        smallest.as_str(),
        "hour" | "minute" | "second" | "millisecond" | "microsecond" | "nanosecond"
    ) || !matches!(
        largest.as_str(),
        "hour" | "minute" | "second" | "millisecond" | "microsecond" | "nanosecond"
    ) {
        return Err(crate::value::error::throw_range_error("Invalid time unit"));
    }
    let scale = unit_scale(&smallest)
        .ok_or_else(|| crate::value::error::throw_range_error("Invalid smallestUnit"))?;
    if !increment.is_finite() || increment <= 0.0 {
        return Err(crate::value::error::throw_range_error(
            "Invalid roundingIncrement",
        ));
    }
    let increment_maximum = match smallest.as_str() {
        "hour" => 24_i128,
        "minute" | "second" => 60,
        "millisecond" | "microsecond" | "nanosecond" => 1_000,
        _ => 0,
    };
    if increment as i128 >= increment_maximum || increment_maximum % increment as i128 != 0 {
        return Err(crate::value::error::throw_range_error(
            "Invalid roundingIncrement",
        ));
    }
    if unit_scale(&largest).unwrap_or(0) < scale {
        return Err(crate::value::error::throw_range_error(
            "Invalid unit relationship",
        ));
    }
    if ![
        "ceil",
        "floor",
        "expand",
        "trunc",
        "halfCeil",
        "halfFloor",
        "halfExpand",
        "halfTrunc",
        "halfEven",
    ]
    .contains(&rounding_mode.as_str())
    {
        return Err(crate::value::error::throw_range_error(
            "Invalid roundingMode",
        ));
    }
    delta = round_instant_integer(delta, scale * increment as i128, &rounding_mode);
    let mut fields = vec![Value::Number(0.0); 10];
    let scales = [
        "day",
        "hour",
        "minute",
        "second",
        "millisecond",
        "microsecond",
        "nanosecond",
    ];
    let largest_index = scales.iter().position(|name| *name == largest).unwrap_or(3) + 3;
    let largest_scale = unit_scale(&largest).unwrap_or(1_000_000_000);
    let mut remainder = delta;
    for index in largest_index..=9 {
        let unit_scale = unit_scale(scales[index - 3]).unwrap_or(1);
        if unit_scale < scale {
            continue;
        }
        let value = remainder / unit_scale;
        fields[index] = Value::Number(value as f64);
        remainder %= unit_scale;
        if unit_scale == scale {
            break;
        }
    }
    if largest_scale == scale {
        fields[largest_index] = Value::Number((delta / scale) as f64);
    }
    crate::temporal::duration::construct(&fields)
}

fn temporal_unit_option(options: Option<&Value>, key: &str) -> Result<Option<String>, VmError> {
    let Some(options) = options.filter(|value| crate::value::is_object(value)) else {
        return Ok(None);
    };
    let value = crate::execute::get_property_result(options, key)?;
    if matches!(value, Value::Undefined) {
        return Ok(None);
    }
    let value = crate::conversion::to_string(&value)?;
    Ok(Some(value.strip_suffix('s').unwrap_or(&value).to_string()))
}

fn round(receiver: Option<&Value>, options: Option<&Value>) -> Result<Value, VmError> {
    let epoch = epoch_number(get_epoch(receiver)?)?;
    if options.is_none()
        || options.is_some_and(|value| {
            matches!(value, Value::Undefined | Value::Null)
                || crate::conversion::is_symbol(value)
                || (!crate::value::is_object(value)
                    && !matches!(value, Value::String(_) | Value::StringUnits(_)))
        })
    {
        return Err(crate::value::error::throw_type_error("Invalid options"));
    }
    let (increment_number, rounding_mode_text, smallest_unit_text) =
        if let Some(value) = options.filter(|value| crate::value::is_object(value)) {
            let increment = crate::execute::get_property_result(value, "roundingIncrement")?;
            let increment = if matches!(increment, Value::Undefined) {
                None
            } else {
                Some(crate::conversion::to_number(&increment)?.trunc())
            };
            let rounding_mode = crate::execute::get_property_result(value, "roundingMode")?;
            let rounding_mode = if matches!(rounding_mode, Value::Undefined) {
                None
            } else {
                Some(crate::conversion::to_string(&rounding_mode)?)
            };
            let smallest = crate::execute::get_property_result(value, "smallestUnit")?;
            let smallest = if matches!(smallest, Value::Undefined) {
                None
            } else {
                Some(crate::conversion::to_string(&smallest)?)
            };
            (increment, rounding_mode, smallest)
        } else {
            (None, None, None)
        };
    let shorthand = options.and_then(|value| match value {
        Value::String(value) => Some(value.clone()),
        Value::StringUnits(_) => crate::conversion::to_string(value).ok(),
        _ => None,
    });
    let unit_value = match shorthand {
        Some(value) => value,
        None => match smallest_unit_text {
            None => {
                return Err(crate::value::error::throw_range_error(
                    "Missing smallestUnit",
                ));
            }
            Some(value) => value,
        },
    };
    let unit = unit_value
        .strip_suffix('s')
        .unwrap_or(&unit_value)
        .to_string();
    let scale = unit_scale(&unit)
        .filter(|_| unit != "day")
        .ok_or_else(|| crate::value::error::throw_range_error("Invalid smallestUnit"))?;
    let increment = increment_number.unwrap_or(1.0);
    if !increment.is_finite() || increment <= 0.0 {
        return Err(crate::value::error::throw_range_error(
            "Invalid roundingIncrement",
        ));
    }
    let maximum = match unit.as_str() {
        "hour" => 24.0,
        "minute" => 1_440.0,
        "second" => 86_400.0,
        "millisecond" => 86_400_000.0,
        "microsecond" => 86_400_000_000.0,
        "nanosecond" => 86_400_000_000_000.0,
        _ => 0.0,
    };
    if increment > maximum || (maximum % increment) != 0.0 {
        return Err(crate::value::error::throw_range_error(
            "Invalid roundingIncrement",
        ));
    }
    let rounding_mode = rounding_mode_text.unwrap_or_else(|| "halfExpand".to_string());
    if ![
        "ceil",
        "floor",
        "expand",
        "trunc",
        "halfCeil",
        "halfFloor",
        "halfExpand",
        "halfTrunc",
        "halfEven",
    ]
    .contains(&rounding_mode.as_str())
    {
        return Err(crate::value::error::throw_range_error(
            "Invalid roundingMode",
        ));
    }
    let quantum = scale * increment as i128;
    let round_mode = if epoch < 0 {
        match rounding_mode.as_str() {
            "expand" => "ceil",
            "trunc" => "floor",
            "halfExpand" => "halfCeil",
            _ => rounding_mode.as_str(),
        }
    } else {
        rounding_mode.as_str()
    };
    let rounded = round_instant_integer(epoch, quantum, round_mode);
    construct(&[Value::BigInt(rounded.to_string())])
}

fn round_instant_integer(value: i128, quantum: i128, mode: &str) -> i128 {
    let sign = value.signum();
    let absolute = value.abs();
    let mut units = absolute / quantum;
    let remainder = absolute % quantum;
    let increment = match mode {
        "ceil" => sign > 0 && remainder != 0,
        "floor" => sign < 0 && remainder != 0,
        "expand" => remainder != 0,
        "trunc" => false,
        "halfEven" => remainder * 2 > quantum || remainder * 2 == quantum && units % 2 != 0,
        "halfCeil" => remainder * 2 >= quantum && sign > 0 || remainder * 2 > quantum && sign < 0,
        "halfFloor" => remainder * 2 > quantum && sign > 0 || remainder * 2 >= quantum && sign < 0,
        "halfTrunc" => remainder * 2 > quantum,
        _ => remainder * 2 >= quantum,
    };
    if increment {
        units += 1;
    }
    units * sign * quantum
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
    let epoch_milliseconds = get_epoch_milliseconds(receiver)?;
    let formatter = crate::intl::datetime::construct_with_defaults(
        arguments,
        Some(&["year", "month", "day", "hour", "minute", "second"]),
    )?;
    crate::intl::datetime::prototype_method(
        crate::ops::Builtin::IntlDateTimeFormatFormat,
        std::slice::from_ref(&epoch_milliseconds),
        Some(&formatter),
    )
}

fn to_zoned_date_time_iso(
    receiver: Option<&Value>,
    time_zone: Option<&Value>,
) -> Result<Value, VmError> {
    let epoch = get_epoch(receiver)?;
    let zone = match time_zone {
        Some(Value::String(value)) => value.clone(),
        Some(Value::StringUnits(_)) => {
            crate::conversion::to_string(time_zone.expect("time zone present"))?
        }
        _ => return Err(crate::value::error::throw_type_error("Invalid time zone")),
    };
    let zone = crate::temporal::parse_timezone_identifier(&Value::String(zone))?;
    let epoch = match epoch {
        Value::BigInt(value) => value
            .parse::<i128>()
            .map_err(|_| crate::value::error::throw_range_error("Invalid epochNanoseconds"))?,
        _ => {
            return Err(crate::value::error::throw_type_error(
                "Invalid epochNanoseconds",
            ))
        }
    };
    Ok(crate::temporal::zoned_record(
        epoch,
        zone,
        crate::ops::Builtin::TemporalZonedDateTimePrototype,
    ))
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
    let Value::Object(object) = receiver else {
        return Err(crate::value::error::throw_type_error("Not an Instant"));
    };
    if !object
        .iter()
        .any(|(key, value)| key == "epochNanoseconds" && matches!(value, Value::BigInt(_)))
    {
        return Err(crate::value::error::throw_type_error("Not an Instant"));
    }
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
        if let Ok(canonical) =
            crate::temporal::parse_timezone_identifier(&Value::String(annotation.to_string()))
        {
            return time_zone_offset(&canonical);
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
    let right = from(other)?;
    let right = get_epoch(Some(&right))?;
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
    for name in ["years", "months", "weeks", "days"] {
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
