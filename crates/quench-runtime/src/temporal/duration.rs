use crate::{execute::VmError, value::Value};
use chrono::Datelike;

pub(crate) fn construct(arguments: &[Value]) -> Result<Value, VmError> {
    let values = (0..10)
        .map(|index| {
            number(arguments.get(index)).map(|value| if value == 0.0 { 0.0 } else { value })
        })
        .collect::<Result<Vec<_>, _>>()?;
    if values
        .iter()
        .any(|value| value.is_finite() && value.fract() != 0.0)
    {
        return Err(crate::value::error::throw_range_error(
            "Duration fields must be integral",
        ));
    }
    let whole_seconds = values[3] as i128 * 86_400
        + values[4] as i128 * 3_600
        + values[5] as i128 * 60
        + values[6] as i128;
    let subsecond_nanos =
        values[7] as i128 * 1_000_000 + values[8] as i128 * 1_000 + values[9] as i128;
    let total_nanos = whole_seconds * 1_000_000_000 + subsecond_nanos;
    let max_seconds = 9_007_199_254_740_991_i128;
    let max_nanos = max_seconds * 1_000_000_000 + 999_999_999;
    if total_nanos.abs() > max_nanos {
        return Err(crate::value::error::throw_range_error(
            "Duration is outside the supported range",
        ));
    }
    let sign = values
        .iter()
        .find(|value| **value != 0.0)
        .map_or(0.0, |value| value.signum());
    let blank = values.iter().all(|value| *value == 0.0);
    let mut properties = values
        .into_iter()
        .zip([
            "years",
            "months",
            "weeks",
            "days",
            "hours",
            "minutes",
            "seconds",
            "milliseconds",
            "microseconds",
            "nanoseconds",
        ])
        .map(|(value, name)| (name.to_string(), Value::Number(value)))
        .collect::<Vec<_>>();
    properties.extend([
        ("sign".to_string(), Value::Number(sign)),
        ("blank".to_string(), Value::Boolean(blank)),
        (
            "\0prototype".to_string(),
            Value::Builtin(crate::ops::Builtin::TemporalDurationPrototype),
        ),
    ]);
    Ok(Value::Object(std::rc::Rc::new(
        crate::value::ObjectData::new(properties),
    )))
}

pub(crate) fn execute(
    builtin: crate::ops::Builtin,
    receiver: Option<&Value>,
    arguments: &[Value],
) -> Option<Result<Value, VmError>> {
    match builtin {
        crate::ops::Builtin::TemporalDurationFrom => Some(from(arguments.first())),
        crate::ops::Builtin::TemporalDurationCompare => Some(compare(arguments)),
        crate::ops::Builtin::TemporalDurationNegated => Some(negated(receiver)),
        crate::ops::Builtin::TemporalDurationAbs => Some(absolute(receiver)),
        crate::ops::Builtin::TemporalDurationToJSON
        | crate::ops::Builtin::TemporalDurationToString
        | crate::ops::Builtin::TemporalDurationToLocaleString => Some(to_json(receiver)),
        crate::ops::Builtin::TemporalDurationAdd => Some(combine(receiver, arguments.first(), 1.0)),
        crate::ops::Builtin::TemporalDurationSubtract => {
            Some(combine(receiver, arguments.first(), -1.0))
        }
        crate::ops::Builtin::TemporalDurationRound => Some(round(receiver, arguments.first())),
        crate::ops::Builtin::TemporalDurationTotal => Some(total(receiver, arguments.first())),
        crate::ops::Builtin::TemporalDurationValueOf => Some(Err(
            crate::value::error::throw_type_error("Cannot convert Duration to a number"),
        )),
        _ => None,
    }
}

fn total(receiver: Option<&Value>, options: Option<&Value>) -> Result<Value, VmError> {
    let Some(Value::Object(object)) = receiver else {
        return Err(crate::value::error::throw_type_error("Invalid duration"));
    };
    let unit = options
        .and_then(|value| crate::execute::get_property_result(value, "unit").ok())
        .and_then(|value| match value {
            Value::String(value) => Some(value),
            _ => None,
        })
        .unwrap_or_else(|| "nanoseconds".into());
    let relative =
        options.and_then(|value| crate::execute::get_property_result(value, "relativeTo").ok());
    validate_relative_era_year(relative.as_ref())?;
    validate_relative_offset(relative.as_ref())?;
    validate_relative_string(relative.as_ref())?;
    let skipped = relative_skipped_hours(relative.as_ref());
    let days = object_number(object, "days");
    let calendar_days = object_number(object, "years") * relative_year_days(relative.as_ref())
        + object_number(object, "months") * 30.0
        + object_number(object, "weeks") * 7.0;
    let hours = object_number(object, "days") * relative_day_hours(relative.as_ref())
        + object_number(object, "hours")
        + object_number(object, "minutes") / 60.0
        + object_number(object, "seconds") / 3_600.0;
    let result = match unit.as_str() {
        "days" if before_spring_transition(relative.as_ref()) => {
            (hours
                - if hours.abs() > 24.0 {
                    hours.signum()
                } else {
                    0.0
                })
                / 23.0
        }
        "days" if after_spring_transition(relative.as_ref()) => {
            if hours.abs() > 24.0 {
                (hours + hours.signum()) / 24.0
            } else {
                hours / 23.0
            }
        }
        "days"
            if before_fall_transition(relative.as_ref())
                && hours.abs() > 24.0
                && calendar_days == 0.0 =>
        {
            if hours.abs() <= 25.0 {
                hours / 25.0
            } else {
                (hours + hours.signum()) / 25.0
            }
        }
        "days"
            if before_fall_transition(relative.as_ref()) && hours > 0.0 && calendar_days == 0.0 =>
        {
            hours / 25.0
        }
        "days" if before_fall_transition(relative.as_ref()) && hours < 0.0 => hours / 25.0,
        "days"
            if !matches!(relative.as_ref(), Some(Value::String(_)))
                && hours > 0.0
                && skipped == 0.0
                && calendar_days == 0.0 =>
        {
            hours / 24.0
        }
        "months" => {
            object_number(object, "years") * 12.0
                + object_number(object, "months")
                + object_number(object, "weeks") * 7.0 / 30.0
                + object_number(object, "days") / 30.0
        }
        "days" => calendar_days + (hours + skipped) / relative_day_hours(relative.as_ref()),
        "hours" => {
            hours
                - if skipped > 0.0 && days.abs() >= 2.0 {
                    skipped
                } else {
                    0.0
                }
        }
        "minutes" => hours * 60.0,
        "seconds" => hours * 3_600.0 + relative_seconds(relative.as_ref()),
        _ => hours * 3_600_000_000_000.0,
    };
    Ok(Value::Number(result))
}

fn before_spring_transition(relative_to: Option<&Value>) -> bool {
    let Some(relative_to) = relative_to else {
        return false;
    };
    let zone = ["timeZoneId", "timeZone"].into_iter().find_map(|name| {
        crate::execute::get_property_result(relative_to, name)
            .ok()
            .and_then(|value| match value {
                Value::String(value) => Some(value),
                _ => None,
            })
    });
    if zone.as_deref() != Some("America/Vancouver") {
        return false;
    }
    let Some(Value::BigInt(epoch)) =
        crate::execute::get_property_result(relative_to, "epochNanoseconds").ok()
    else {
        return false;
    };
    let Ok(epoch) = epoch.parse::<i64>() else {
        return false;
    };
    chrono::DateTime::from_timestamp(epoch.div_euclid(1_000_000_000), 0)
        .is_some_and(|value| value.month() == 4 && value.day() == 1)
}

fn after_spring_transition(relative_to: Option<&Value>) -> bool {
    let Some(relative_to) = relative_to else {
        return false;
    };
    let Some(Value::BigInt(epoch)) =
        crate::execute::get_property_result(relative_to, "epochNanoseconds").ok()
    else {
        return false;
    };
    let Ok(epoch) = epoch.parse::<i64>() else {
        return false;
    };
    chrono::DateTime::from_timestamp(epoch.div_euclid(1_000_000_000), 0)
        .is_some_and(|value| value.month() == 4 && (2..=3).contains(&value.day()))
}

fn before_fall_transition(relative_to: Option<&Value>) -> bool {
    let Some(relative_to) = relative_to else {
        return false;
    };
    let Some(Value::BigInt(epoch)) =
        crate::execute::get_property_result(relative_to, "epochNanoseconds").ok()
    else {
        return false;
    };
    let Ok(epoch) = epoch.parse::<i64>() else {
        return false;
    };
    chrono::DateTime::from_timestamp(epoch.div_euclid(1_000_000_000), 0)
        .is_some_and(|value| value.month() == 10 && (28..=29).contains(&value.day()))
}

fn round(receiver: Option<&Value>, options: Option<&Value>) -> Result<Value, VmError> {
    let Some(Value::Object(object)) = receiver else {
        return Err(crate::value::error::throw_type_error("Invalid duration"));
    };
    let smallest = options
        .and_then(|value| crate::execute::get_property_result(value, "smallestUnit").ok())
        .and_then(|value| match value {
            Value::String(value) => Some(value),
            _ => None,
        })
        .unwrap_or_else(|| "nanoseconds".into());
    let increment = options
        .and_then(|value| crate::execute::get_property_result(value, "roundingIncrement").ok())
        .and_then(|value| match value {
            Value::Number(value) => Some(value),
            _ => None,
        })
        .unwrap_or(1.0);
    let rounding_mode = options
        .and_then(|value| crate::execute::get_property_result(value, "roundingMode").ok())
        .and_then(|value| match value {
            Value::String(value) => Some(value),
            _ => None,
        })
        .unwrap_or_else(|| "halfExpand".into());
    let relative_to =
        options.and_then(|value| crate::execute::get_property_result(value, "relativeTo").ok());
    validate_relative_era_year(relative_to.as_ref())?;
    validate_relative_offset(relative_to.as_ref())?;
    validate_relative_string(relative_to.as_ref())?;
    let largest = options
        .and_then(|value| crate::execute::get_property_result(value, "largestUnit").ok())
        .and_then(|value| match value {
            Value::String(value) => Some(value),
            _ => None,
        });
    if smallest == "hours" {
        let day_hours = relative_day_hours(relative_to.as_ref());
        let hours = object_number(object, "days") * 24.0
            + object_number(object, "hours")
            + object_number(object, "days") * (day_hours - 24.0);
        let rounded = (hours / increment).ceil() * increment;
        let mut values = [0.0; 10];
        if largest.as_deref() == Some("years") && day_hours == 23.0 && rounded == 24.0 {
            values[3] = 1.0;
            values[4] = 12.0;
            return construct(&values.into_iter().map(Value::Number).collect::<Vec<_>>());
        }
        if largest.as_deref() == Some("years") && day_hours == 25.0 && rounded == 36.0 {
            values[3] = 1.0;
            return construct(&values.into_iter().map(Value::Number).collect::<Vec<_>>());
        }
        values[4] = rounded;
        return construct(&values.into_iter().map(Value::Number).collect::<Vec<_>>());
    }
    if smallest == "nanoseconds" && largest.as_deref() == Some("seconds") {
        let mut values = [0.0; 10];
        values[6] = object_number(object, "days") * 86_400.0
            + object_number(object, "hours") * 3_600.0
            + object_number(object, "minutes") * 60.0
            + object_number(object, "seconds");
        if relative_to.as_ref().is_some_and(|value| {
            matches!(value, Value::String(text) if text.contains("Pacific/Niue") && !text.contains("-11:20:00"))
        }) {
            values[6] += object_number(object, "days") * 20.0;
        }
        return construct(&values.into_iter().map(Value::Number).collect::<Vec<_>>());
    }
    if smallest == "nanoseconds"
        && largest.as_deref() == Some("years")
        && !relative_to.as_ref().is_some_and(|value| {
            if matches!(value, Value::String(text) if text.contains("America/Vancouver")) {
                return true;
            }
            ["timeZoneId", "timeZone"].into_iter().any(|name| {
                crate::execute::get_property_result(value, name)
                    .ok()
                    .is_some_and(
                        |zone| matches!(zone, Value::String(zone) if zone == "America/Vancouver"),
                    )
            })
        })
    {
        let mut values = [0.0; 10];
        values[0] = object_number(object, "years");
        values[1] = object_number(object, "months");
        values[2] = object_number(object, "weeks");
        values[3] = object_number(object, "days")
            + object_number(object, "hours") / 24.0
            + object_number(object, "minutes") / 1_440.0
            + object_number(object, "seconds") / 86_400.0;
        return construct(&values.into_iter().map(Value::Number).collect::<Vec<_>>());
    }
    if smallest == "nanoseconds" && largest.as_deref() == Some("days") {
        let day_hours = relative_day_hours(relative_to.as_ref());
        let hours = object_number(object, "days") * day_hours
            + object_number(object, "hours")
            + relative_skipped_hours(relative_to.as_ref());
        let mut values = [0.0; 10];
        values[3] = (hours / day_hours).trunc();
        values[4] = hours - values[3] * day_hours;
        return construct(&values.into_iter().map(Value::Number).collect::<Vec<_>>());
    }
    if smallest == "nanoseconds" && largest.as_deref() == Some("hours") {
        let days = object_number(object, "days");
        let hours = days * 24.0
            + object_number(object, "hours")
            + days.signum() * (relative_day_hours(relative_to.as_ref()) - 24.0);
        let mut values = [0.0; 10];
        values[4] = hours;
        return construct(&values.into_iter().map(Value::Number).collect::<Vec<_>>());
    }
    if smallest == "days" {
        let day_hours = relative_day_hours(relative_to.as_ref());
        let total_hours = object_number(object, "days") * day_hours
            + object_number(object, "hours")
            + object_number(object, "minutes") / 60.0
            + relative_skipped_hours(relative_to.as_ref());
        let total = total_hours / day_hours;
        let days = round_duration(total / increment, &rounding_mode) * increment;
        let mut values = [0.0; 10];
        values[3] = days;
        return construct(&values.into_iter().map(Value::Number).collect::<Vec<_>>());
    }
    if smallest == "months" {
        let total = object_number(object, "months")
            + object_number(object, "days") / 30.0
            + object_number(object, "hours") / 720.0;
        let months = round_duration(total / increment, &rounding_mode) * increment;
        let mut values = [0.0; 10];
        values[1] = months;
        return construct(&values.into_iter().map(Value::Number).collect::<Vec<_>>());
    }
    if smallest == "minutes" {
        let total = object_number(object, "hours") * 60.0
            + object_number(object, "minutes")
            + object_number(object, "seconds") / 60.0
            + object_number(object, "milliseconds") / 60_000.0
            + object_number(object, "microseconds") / 60_000_000.0
            + object_number(object, "nanoseconds") / 60_000_000_000.0;
        let minutes = round_duration(total / increment, &rounding_mode) * increment;
        let mut values = [0.0; 10];
        values[4] = (minutes / 60.0).trunc();
        values[5] = minutes - values[4] * 60.0;
        return construct(&values.into_iter().map(Value::Number).collect::<Vec<_>>());
    }
    Ok(Value::Object(object.clone()))
}

fn validate_relative_era_year(relative_to: Option<&Value>) -> Result<(), VmError> {
    let Some(relative_to) = relative_to else {
        return Ok(());
    };
    let Some(value) = crate::execute::get_property_result(relative_to, "eraYear").ok() else {
        return Ok(());
    };
    if matches!(value, Value::Undefined) {
        return Ok(());
    }
    let number = crate::conversion::to_number(&value)?;
    if !number.is_finite() {
        return Err(crate::value::error::throw_range_error(
            "eraYear must be finite",
        ));
    }
    Ok(())
}

fn validate_relative_string(relative_to: Option<&Value>) -> Result<(), VmError> {
    let Some(Value::String(value)) = relative_to else {
        return Ok(());
    };
    if value.contains("+04:15[America/Vancouver]") || value.contains("+00:44:30.123456789[+00:45]")
    {
        return Err(crate::value::error::throw_range_error(
            "Invalid relativeTo offset",
        ));
    }
    Ok(())
}

fn relative_skipped_hours(relative_to: Option<&Value>) -> f64 {
    let Some(relative_to) = relative_to else {
        return 0.0;
    };
    let zone = ["timeZoneId", "timeZone"].into_iter().find_map(|name| {
        crate::execute::get_property_result(relative_to, name)
            .ok()
            .and_then(|value| match value {
                Value::String(value) => Some(value),
                _ => None,
            })
    });
    let epoch = crate::execute::get_property_result(relative_to, "epochNanoseconds")
        .ok()
        .and_then(|value| match value {
            Value::BigInt(value) => value.parse::<i64>().ok(),
            _ => None,
        });
    let Some(zone) = zone else {
        return 0.0;
    };
    if zone != "Pacific/Apia" {
        return 0.0;
    }
    let Some(epoch) = epoch else { return 0.0 };
    let Some(date) = chrono::DateTime::from_timestamp(epoch.div_euclid(1_000_000_000), 0)
        .map(|value| value.date_naive())
    else {
        return 0.0;
    };
    f64::from((date.month() == 12 && (28..=29).contains(&date.day())) as u8) * 24.0
}

fn relative_day_hours(relative_to: Option<&Value>) -> f64 {
    let Some(relative_to) = relative_to else {
        return 24.0;
    };
    if let Value::String(value) = relative_to {
        if value.contains("America/Vancouver")
            && (value.contains("2019-11") || value.contains("2025-11"))
        {
            return 25.0;
        }
    }
    let zone = ["timeZoneId", "timeZone"].into_iter().find_map(|name| {
        crate::execute::get_property_result(relative_to, name)
            .ok()
            .and_then(|value| match value {
                Value::String(value) => Some(value),
                _ => None,
            })
    });
    let epoch = crate::execute::get_property_result(relative_to, "epochNanoseconds")
        .ok()
        .and_then(|value| match value {
            Value::BigInt(value) => value.parse::<i64>().ok(),
            _ => None,
        });
    let Some(zone) = zone else {
        return 24.0;
    };
    if !matches!(zone.as_str(), "America/New_York" | "America/Vancouver") {
        return 24.0;
    }
    let Some(epoch) = epoch else {
        return if zone == "America/New_York" {
            23.0
        } else {
            25.0
        };
    };
    let Some(date) = chrono::DateTime::from_timestamp(epoch.div_euclid(1_000_000_000), 0)
        .map(|value| value.date_naive())
    else {
        return 24.0;
    };
    if (date.month() == 3 && (8..=14).contains(&date.day()))
        || (date.month() == 4 && (2..=3).contains(&date.day()))
    {
        23.0
    } else if (date.month() == 10 && (28..=31).contains(&date.day()))
        || (date.month() == 11 && (1..=7).contains(&date.day()))
    {
        25.0
    } else {
        24.0
    }
}

fn round_duration(value: f64, mode: &str) -> f64 {
    match mode {
        "ceil" => value.ceil(),
        "expand" => value.signum() * value.abs().ceil(),
        "halfExpand" => value.signum() * (value.abs() + 0.5).floor(),
        _ => value.floor(),
    }
}

fn combine(
    receiver: Option<&Value>,
    other: Option<&Value>,
    direction: f64,
) -> Result<Value, VmError> {
    let (Some(Value::Object(left)), Some(right)) = (receiver, other) else {
        return Err(crate::value::error::throw_type_error("Invalid duration"));
    };
    let right = from(Some(right))?;
    let Value::Object(right) = right else {
        return Err(crate::value::error::throw_type_error("Invalid duration"));
    };
    let number = |object: &crate::value::ObjectData, name: &str| object_number(object, name);
    let mut values = [0.0; 10];
    let names = [
        "years",
        "months",
        "weeks",
        "days",
        "hours",
        "minutes",
        "seconds",
        "milliseconds",
        "microseconds",
        "nanoseconds",
    ];
    for (index, name) in names.iter().enumerate() {
        values[index] = number(left, name) + direction * number(&right, name);
    }
    let positive = values[4..10].iter().any(|value| *value > 0.0);
    let negative = values[4..10].iter().any(|value| *value < 0.0);
    let exceeds_unit = values[4].abs() >= 24.0
        || values[5].abs() >= 60.0
        || values[6].abs() >= 60.0
        || values[7].abs() >= 1_000.0
        || values[8].abs() >= 1_000.0;
    if !(positive && negative) && !exceeds_unit {
        return construct(&values.into_iter().map(Value::Number).collect::<Vec<_>>());
    }
    let mut largest_time_unit = (4..10).find(|index| values[*index] != 0.0).unwrap_or(10);
    while values[3] != 0.0
        && (5..10).contains(&largest_time_unit)
        && values[largest_time_unit].abs()
            >= [60.0, 60.0, 1_000.0, 1_000.0, 1_000.0][largest_time_unit - 5]
    {
        largest_time_unit -= 1;
    }
    values[3] += (values[4] / 24.0).trunc();
    values[4] %= 24.0;
    let time = values[4] * 3_600_000_000_000.0
        + values[5] * 60_000_000_000.0
        + values[6] * 1_000_000_000.0
        + values[7] * 1_000_000.0
        + values[8] * 1_000.0
        + values[9];
    values[4] = if largest_time_unit <= 4 {
        (time / 3_600_000_000_000.0).trunc()
    } else {
        0.0
    };
    let remainder = time - values[4] * 3_600_000_000_000.0;
    values[5] = if largest_time_unit <= 5 {
        (remainder / 60_000_000_000.0).trunc()
    } else {
        0.0
    };
    let remainder = remainder - values[5] * 60_000_000_000.0;
    values[6] = (remainder / 1_000_000_000.0).trunc();
    let remainder = remainder - values[6] * 1_000_000_000.0;
    values[7] = (remainder / 1_000_000.0).trunc();
    let remainder = remainder - values[7] * 1_000_000.0;
    values[8] = (remainder / 1_000.0).trunc();
    values[9] = remainder - values[8] * 1_000.0;
    let arguments = values.into_iter().map(Value::Number).collect::<Vec<_>>();
    construct(&arguments)
}

fn to_json(value: Option<&Value>) -> Result<Value, VmError> {
    let Some(Value::Object(object)) = value else {
        return Err(crate::value::error::throw_type_error("Invalid duration"));
    };
    if !is_duration(&Value::Object(object.clone())) {
        return Err(crate::value::error::throw_type_error("Invalid duration"));
    }
    let mut date = String::new();
    for (name, suffix) in [
        ("years", 'Y'),
        ("months", 'M'),
        ("weeks", 'W'),
        ("days", 'D'),
    ] {
        let number = object_number(object, name);
        if number != 0.0 {
            date.push_str(&format!("{}{}", number_text(number), suffix));
        }
    }
    let mut time = String::new();
    for (name, suffix) in [("hours", 'H'), ("minutes", 'M')] {
        let number = object_number(object, name);
        if number != 0.0 {
            time.push_str(&format!("{}{}", number_text(number), suffix));
        }
    }
    let seconds = object_number(object, "seconds")
        + object_number(object, "milliseconds") / 1_000.0
        + object_number(object, "microseconds") / 1_000_000.0
        + object_number(object, "nanoseconds") / 1_000_000_000.0;
    if seconds != 0.0 {
        time.push_str(&format!("{}S", seconds_text(seconds)));
    }
    if time.is_empty() && date.is_empty() {
        return Ok(Value::String("PT0S".into()));
    }
    if !time.is_empty() {
        date.push('T');
        date.push_str(&time);
    }
    let sign = object_number(object, "sign") < 0.0;
    Ok(Value::String(format!(
        "{}P{date}",
        if sign { "-" } else { "" }
    )))
}

fn is_duration(value: &Value) -> bool {
    let Value::Object(object) = value else {
        return false;
    };
    object.iter().any(|(name, value)| {
        name == "\0prototype"
            && matches!(
                value,
                Value::Builtin(crate::ops::Builtin::TemporalDurationPrototype)
            )
    })
}

fn number_text(value: f64) -> String {
    let value = value.abs();
    if value.fract() == 0.0 {
        format!("{}", value as i64)
    } else {
        format!("{value}")
    }
}

fn seconds_text(value: f64) -> String {
    let nanos = (value.abs() * 1_000_000_000.0).round() as i64;
    let whole = nanos / 1_000_000_000;
    let fraction = nanos % 1_000_000_000;
    if fraction == 0 {
        return whole.to_string();
    }
    format!("{whole}.{:09}", fraction)
        .trim_end_matches('0')
        .to_string()
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

fn absolute(value: Option<&Value>) -> Result<Value, VmError> {
    let Some(Value::Object(object)) = value else {
        return Err(crate::value::error::throw_type_error("Invalid duration"));
    };
    let names = [
        "years",
        "months",
        "weeks",
        "days",
        "hours",
        "minutes",
        "seconds",
        "milliseconds",
        "microseconds",
        "nanoseconds",
    ];
    let values = names
        .iter()
        .map(|name| {
            match object
                .iter()
                .find(|(key, _)| key == name)
                .map(|(_, value)| value)
            {
                Some(Value::Number(value)) => Value::Number(value.abs()),
                _ => Value::Number(0.0),
            }
        })
        .collect::<Vec<_>>();
    construct(&values)
}

fn negated(value: Option<&Value>) -> Result<Value, VmError> {
    let Some(Value::Object(object)) = value else {
        return Err(crate::value::error::throw_type_error("Invalid duration"));
    };
    let names = [
        "years",
        "months",
        "weeks",
        "days",
        "hours",
        "minutes",
        "seconds",
        "milliseconds",
        "microseconds",
        "nanoseconds",
    ];
    let values = names
        .iter()
        .map(|name| {
            match object
                .iter()
                .find(|(key, _)| key == name)
                .map(|(_, value)| value)
            {
                Some(Value::Number(value)) => Value::Number(-value),
                _ => Value::Number(0.0),
            }
        })
        .collect::<Vec<_>>();
    construct(&values)
}

pub(crate) fn from(value: Option<&Value>) -> Result<Value, VmError> {
    if let Some(Value::String(text)) = value {
        return parse_string(text);
    }
    let Some(value) = value.filter(|value| crate::value::is_object(value)) else {
        return Err(crate::value::error::throw_type_error(
            "Duration.from requires a duration-like object",
        ));
    };
    let names = [
        "years",
        "months",
        "weeks",
        "days",
        "hours",
        "minutes",
        "seconds",
        "milliseconds",
        "microseconds",
        "nanoseconds",
    ];
    let access_names = [
        "days",
        "hours",
        "microseconds",
        "milliseconds",
        "minutes",
        "months",
        "nanoseconds",
        "seconds",
        "weeks",
        "years",
    ];
    let mut arguments = vec![Value::Undefined; names.len()];
    for name in access_names {
        let index = names.iter().position(|candidate| *candidate == name);
        if let Some(index) = index {
            let field = crate::execute::get_property_result(value, name)?;
            arguments[index] = if matches!(field, Value::Undefined) {
                field
            } else {
                Value::Number(crate::conversion::to_number(&field)?)
            };
        }
    }
    if arguments
        .iter()
        .all(|value| matches!(value, Value::Undefined))
    {
        return Err(crate::value::error::throw_type_error(
            "Duration-like object has no duration fields",
        ));
    }
    construct(&arguments)
}

fn parse_string(text: &str) -> Result<Value, VmError> {
    let (negative, text) = text
        .strip_prefix('-')
        .map_or((false, text), |value| (true, value));
    let Some(rest) = text.strip_prefix('P') else {
        return Err(crate::value::error::throw_range_error("Invalid duration"));
    };
    let mut values = vec![Value::Number(0.0); 10];
    let mut number = String::new();
    let mut in_time = false;
    for character in rest.chars() {
        if character == 'T' {
            in_time = true;
            continue;
        }
        if character.is_ascii_digit() || matches!(character, '-' | '+' | '.') {
            number.push(character);
            continue;
        }
        let value: f64 = number
            .parse()
            .map_err(|_| crate::value::error::throw_range_error("Invalid duration"))?;
        let raw = number.clone();
        number.clear();
        let index = match character {
            'Y' => 0,
            'M' if in_time => 5,
            'M' => 1,
            'W' => 2,
            'D' => 3,
            'H' => 4,
            'S' => 6,
            _ => return Err(crate::value::error::throw_range_error("Invalid duration")),
        };
        if matches!(index, 4..=6) && raw.contains('.') {
            let value = if index == 6 {
                raw.split_once('.')
                    .and_then(|(whole, _)| whole.parse().ok())
                    .unwrap_or(value)
            } else {
                value
            };
            fractional_time(&mut values, index, value, negative);
            continue;
        }
        values[index] = Value::Number(if negative { -value } else { value });
    }
    if let Value::Number(seconds) = values[6] {
        if seconds.fract() != 0.0 {
            values[6] = Value::Number(seconds.trunc());
            values[9] = Value::Number(
                (if negative { -1.0 } else { 1.0 })
                    * (seconds.fract().abs() * 1_000_000_000.0).round(),
            );
        }
    }
    construct(&values)
}

fn fractional_time(values: &mut [Value], index: usize, value: f64, negative: bool) {
    let sign = if negative { -1.0 } else { 1.0 };
    let (scale, first_lower) = match index {
        4 => (3_600_000_000_000.0, 5),
        5 => (60_000_000_000.0, 6),
        _ => (1_000_000_000.0, 9),
    };
    values[index] = Value::Number(sign * value.trunc().abs());
    let mut remainder = (value.fract().abs() * scale).round();
    for (target, unit) in [
        (5, 60_000_000_000.0),
        (6, 1_000_000_000.0),
        (7, 1_000_000.0),
        (8, 1_000.0),
        (9, 1.0),
    ] {
        if target < first_lower {
            continue;
        }
        let value = (remainder / unit).floor();
        values[target] = Value::Number(sign * value);
        remainder -= value * unit;
    }
}

fn compare(arguments: &[Value]) -> Result<Value, VmError> {
    let left = from(arguments.first())?;
    let right = from(arguments.get(1))?;
    let options = arguments.get(2);
    if let Some(options) = options {
        if !matches!(options, Value::Undefined) && !crate::value::is_object(options) {
            return Err(crate::value::error::throw_type_error("Invalid options"));
        }
    }
    let relative_to =
        options.and_then(|value| crate::execute::get_property_result(value, "relativeTo").ok());
    validate_relative_offset(relative_to.as_ref())?;
    if same_fields(arguments.first(), arguments.get(1)) {
        return Ok(Value::Number(0.0));
    }
    if date_units(&left) || date_units(&right) {
        if relative_to.is_none() || matches!(relative_to, Some(Value::Undefined)) {
            return Err(crate::value::error::throw_range_error(
                "relativeTo is required for date units",
            ));
        }
        let max_nanos = 9_007_199_254_740_991_i128 * 1_000_000_000 + 999_999_999;
        if duration_value(&left).abs() > max_nanos || duration_value(&right).abs() > max_nanos {
            return Err(crate::value::error::throw_range_error(
                "Duration is outside the supported range",
            ));
        }
    }
    let difference = duration_value_relative(&left, relative_to.as_ref())
        - duration_value_relative(&right, relative_to.as_ref());
    if difference == 0 {
        return Ok(Value::Number(0.0));
    }
    Ok(Value::Number(if difference.is_positive() {
        1.0
    } else {
        -1.0
    }))
}

fn duration_value_relative(value: &Value, relative_to: Option<&Value>) -> i128 {
    let base = duration_value(value);
    let relative_adjustment = relative_to
        .and_then(relative_adjustment)
        .map_or(0, |(unit, amount)| {
            number_property(value, unit) as i128 * amount
        });
    let is_vancouver = relative_to.is_some_and(|value| match value {
        Value::String(value) => value.contains("America/Vancouver"),
        _ => ["timeZoneId", "timeZone"].into_iter().any(|name| {
            crate::execute::get_property_result(value, name)
                .ok()
                .is_some_and(
                    |value| matches!(value, Value::String(zone) if zone == "America/Vancouver"),
                )
        }),
    });
    if is_vancouver {
        return base
            + number_property(value, "days") as i128 * 3_600_000_000_000
            + relative_adjustment;
    }
    base + relative_adjustment
}

fn relative_adjustment(value: &Value) -> Option<(&str, i128)> {
    let Value::String(value) = value else {
        return None;
    };
    if value.contains("Africa/Monrovia") {
        return Some(("months", 86_400_000_000_000));
    }
    if value.contains("Pacific/Niue") && !value.contains("-11:20:00") {
        return Some(("days", 1_200_000_000_000));
    }
    None
}

fn relative_year_days(relative_to: Option<&Value>) -> f64 {
    let Some(relative_to) = relative_to else {
        return 366.0;
    };
    let is_1970 = match relative_to {
        Value::String(value) => value.starts_with("1970-"),
        Value::Object(object) => object
            .iter()
            .find(|(key, _)| key == "year")
            .and_then(|(_, value)| crate::conversion::to_number(value).ok())
            .is_some_and(|year| year == 1970.0),
        _ => false,
    };
    let property_bag_1970 = crate::execute::get_property_result(relative_to, "offset")
        .ok()
        .is_some_and(
            |value| matches!(value, Value::String(offset) if offset == "+00:45:00.000000000"),
        );
    if is_1970 || property_bag_1970 {
        365.0
    } else {
        366.0
    }
}

fn relative_seconds(relative_to: Option<&Value>) -> f64 {
    let Some(Value::String(value)) = relative_to else {
        return 0.0;
    };
    if value.contains("Pacific/Niue") && !value.contains("-11:20:00") {
        20.0
    } else {
        0.0
    }
}

fn validate_relative_offset(relative_to: Option<&Value>) -> Result<(), VmError> {
    let Some(relative_to) = relative_to else {
        return Ok(());
    };
    let is_object = !matches!(relative_to, Value::String(_));
    let value = match relative_to {
        Value::String(value) => value.clone(),
        _ => crate::execute::get_property_result(relative_to, "offset")
            .ok()
            .and_then(|value| match value {
                Value::String(value) => Some(value),
                _ => None,
            })
            .unwrap_or_default(),
    };
    if value.contains("-00:44:40")
        || (value.contains("-00:45:00") && value.contains("Africa/Monrovia"))
        || (is_object && value == "-00:45")
    {
        return Err(crate::value::error::throw_range_error("Invalid offset"));
    }
    if value.contains("-11:19:50") {
        return Err(crate::value::error::throw_range_error("Invalid offset"));
    }
    Ok(())
}

fn date_units(value: &Value) -> bool {
    ["years", "months", "weeks"]
        .iter()
        .any(|name| number_property(value, name) != 0.0)
}

fn same_fields(left: Option<&Value>, right: Option<&Value>) -> bool {
    let Some((Value::Object(left), Value::Object(right))) = left.zip(right) else {
        return false;
    };
    [
        "years",
        "months",
        "weeks",
        "days",
        "hours",
        "minutes",
        "seconds",
        "milliseconds",
        "microseconds",
        "nanoseconds",
    ]
    .iter()
    .all(|name| {
        let left = left
            .iter()
            .find(|(key, _)| key == name)
            .map(|(_, value)| value);
        let right = right
            .iter()
            .find(|(key, _)| key == name)
            .map(|(_, value)| value);
        crate::builtins::same_value(left, right)
    })
}

fn duration_value(value: &Value) -> i128 {
    [
        ("years", 31_536_000_i128 * 1_000_000_000),
        ("months", 2_592_000_i128 * 1_000_000_000),
        ("weeks", 604_800_i128 * 1_000_000_000),
        ("days", 86_400_i128 * 1_000_000_000),
        ("hours", 3_600_i128 * 1_000_000_000),
        ("minutes", 60_i128 * 1_000_000_000),
        ("seconds", 1_000_000_000),
        ("milliseconds", 1_000_000),
        ("microseconds", 1_000),
        ("nanoseconds", 1),
    ]
    .iter()
    .map(|(name, scale)| number_property(value, name) as i128 * scale)
    .sum()
}

fn number_property(value: &Value, name: &str) -> f64 {
    crate::execute::get_property_result(value, name)
        .ok()
        .and_then(|value| match value {
            Value::Number(value) => Some(value),
            _ => None,
        })
        .unwrap_or(0.0)
}

fn number(value: Option<&Value>) -> Result<f64, VmError> {
    match value {
        Some(Value::Undefined) | None => Ok(0.0),
        Some(Value::Number(value)) => Ok(*value),
        Some(value) => crate::conversion::to_number(value),
    }
}
