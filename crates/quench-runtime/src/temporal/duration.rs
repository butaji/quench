use chrono::{Datelike, NaiveDate};

use crate::{execute::VmError, value::Value};

#[path = "duration_validation.rs"]
mod duration_validation;
use duration_validation::{number, number_property, total_time_out_of_range};

pub(crate) fn construct(arguments: &[Value]) -> Result<Value, VmError> {
    let values = (0..10)
        .map(|index| number(arguments.get(index)))
        .collect::<Result<Vec<_>, _>>()?;
    validate_range(&values)?;
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
        crate::ops::Builtin::TemporalDurationAdd => Some(add(receiver, arguments.first())),
        crate::ops::Builtin::TemporalDurationSubtract => {
            Some(subtract(receiver, arguments.first()))
        }
        crate::ops::Builtin::TemporalDurationAbs => Some(abs(receiver)),
        crate::ops::Builtin::TemporalDurationNegated => Some(negated(receiver)),
        crate::ops::Builtin::TemporalDurationRound => Some(round(receiver, arguments.first())),
        crate::ops::Builtin::TemporalDurationToLocaleString => Some(
            crate::intl::duration::format_temporal_duration(receiver, arguments),
        ),
        crate::ops::Builtin::TemporalDurationToJSON => Some(to_json(receiver)),
        crate::ops::Builtin::TemporalDurationToString => Some(to_json(receiver)),
        crate::ops::Builtin::TemporalDurationYearsGetter => Some(field_getter(receiver, "years")),
        crate::ops::Builtin::TemporalDurationMonthsGetter => Some(field_getter(receiver, "months")),
        crate::ops::Builtin::TemporalDurationWeeksGetter => Some(field_getter(receiver, "weeks")),
        crate::ops::Builtin::TemporalDurationDaysGetter => Some(field_getter(receiver, "days")),
        crate::ops::Builtin::TemporalDurationHoursGetter => Some(field_getter(receiver, "hours")),
        crate::ops::Builtin::TemporalDurationMinutesGetter => {
            Some(field_getter(receiver, "minutes"))
        }
        crate::ops::Builtin::TemporalDurationSecondsGetter => {
            Some(field_getter(receiver, "seconds"))
        }
        crate::ops::Builtin::TemporalDurationMillisecondsGetter => {
            Some(field_getter(receiver, "milliseconds"))
        }
        crate::ops::Builtin::TemporalDurationMicrosecondsGetter => {
            Some(field_getter(receiver, "microseconds"))
        }
        crate::ops::Builtin::TemporalDurationNanosecondsGetter => {
            Some(field_getter(receiver, "nanoseconds"))
        }
        crate::ops::Builtin::TemporalDurationSignGetter => Some(field_getter(receiver, "sign")),
        crate::ops::Builtin::TemporalDurationBlankGetter => {
            Some(boolean_field_getter(receiver, "blank"))
        }
        crate::ops::Builtin::TemporalDurationValueOf => {
            Some(Err(crate::value::error::throw_type_error(
                "Temporal.Duration.prototype.valueOf is not allowed",
            )))
        }
        _ => None,
    }
}

fn field_getter(receiver: Option<&Value>, field: &str) -> Result<Value, VmError> {
    let object = duration_receiver(receiver)?;
    Ok(object
        .iter()
        .find(|(key, _)| key == field)
        .map_or(Value::Number(0.0), |(_, value)| value.clone()))
}

fn boolean_field_getter(receiver: Option<&Value>, field: &str) -> Result<Value, VmError> {
    let object = duration_receiver(receiver)?;
    Ok(object
        .iter()
        .find(|(key, _)| key == field)
        .map_or(Value::Boolean(false), |(_, value)| value.clone()))
}

fn abs(receiver: Option<&Value>) -> Result<Value, VmError> {
    let object = duration_receiver(receiver)?;
    let arguments = absolute_fields(object);
    construct(&arguments)
}

fn add(receiver: Option<&Value>, argument: Option<&Value>) -> Result<Value, VmError> {
    let left = duration_receiver(receiver)?;
    let right = from(argument)?;
    let Value::Object(right) = right else {
        return Err(crate::value::error::throw_type_error("Invalid duration"));
    };
    let fields = balanced_sum(left, &right);
    construct(&fields)
}

fn subtract(receiver: Option<&Value>, argument: Option<&Value>) -> Result<Value, VmError> {
    let right = from(argument)?;
    let Value::Object(right) = right else {
        return Err(crate::value::error::throw_type_error("Invalid duration"));
    };
    let negated = right
        .iter()
        .filter(|(key, _)| !key.starts_with('\0'))
        .map(|(key, value)| {
            let value = match value {
                Value::Number(value) => Value::Number(-value),
                value => value.clone(),
            };
            (key.as_str().to_string(), value)
        })
        .collect::<Vec<_>>();
    let negated = crate::value::ObjectData::new(negated);
    let left = duration_receiver(receiver)?;
    construct(&balanced_sum(left, &negated))
}

fn negated(receiver: Option<&Value>) -> Result<Value, VmError> {
    let object = duration_receiver(receiver)?;
    let fields = [
        "years", "months", "weeks", "days", "hours", "minutes", "seconds",
        "milliseconds", "microseconds", "nanoseconds",
    ]
    .iter()
    .map(|name| Value::Number(-duration_field(object, name) as f64))
    .collect::<Vec<_>>();
    construct(&fields)
}

fn round(receiver: Option<&Value>, options: Option<&Value>) -> Result<Value, VmError> {
    let object = duration_receiver(receiver)?;
    let unit = round_unit(options)?;
    let index = unit_index(unit)?;
    if index < 2 {
        let relative = options.and_then(|value| match value {
            Value::Object(object) => object
                .iter()
                .find(|(key, _)| key == "relativeTo")
                .map(|(_, value)| value),
            _ => None,
        });
        let Some(relative) = relative else {
            return Err(crate::value::error::throw_range_error(
                "relativeTo is required for calendar rounding",
            ));
        };
        return calendar_round(object, relative, index);
    }
    fixed_round(object, options, index)
}

fn fixed_round(
    object: &crate::value::ObjectData,
    options: Option<&Value>,
    index: usize,
) -> Result<Value, VmError> {
    let fields = duration_fields(object);
    let scales = [
        7.0 * 86_400_000_000_000.0,
        86_400_000_000_000.0,
        3_600_000_000_000.0,
        60_000_000_000.0,
        1_000_000_000.0,
        1_000_000.0,
        1_000.0,
        1.0,
    ];
    let first = index.saturating_sub(2);
    let total = fields[2..]
        .iter()
        .zip([
            7.0 * 86_400_000_000_000.0,
            86_400_000_000_000.0,
            3_600_000_000_000.0,
            60_000_000_000.0,
            1_000_000_000.0,
            1_000_000.0,
            1_000.0,
            1.0,
        ])
        .map(|(value, scale)| number_field(value) as f64 * scale)
        .sum::<f64>();
    let increment = rounding_increment(options)?;
    let quantum = scales[first] * increment;
    let rounded_units = round_number(total / quantum, rounding_mode(options)?);
    let mut rounded = vec![Value::Number(0.0); 10];
    rounded[index] = Value::Number(rounded_units * increment);
    let largest = (2..=index)
        .find(|field| number_field(&fields[*field]) != 0)
        .unwrap_or(index);
    if largest < index {
        balance_time_fields(&mut rounded, largest);
    }
    construct(&rounded)
}

fn rounding_increment(options: Option<&Value>) -> Result<f64, VmError> {
    let value = options
        .and_then(|value| match value {
            Value::Object(object) => object
                .iter()
                .find(|(key, _)| key == "roundingIncrement")
                .map(|(_, value)| value),
            _ => None,
        })
        .map(crate::conversion::to_number)
        .transpose()?
        .unwrap_or(1.0);
    (value.is_finite() && value > 0.0)
        .then_some(value)
        .ok_or_else(|| crate::value::error::throw_range_error("Invalid roundingIncrement"))
}

fn rounding_mode(options: Option<&Value>) -> Result<&str, VmError> {
    let value = options
        .and_then(|value| match value {
            Value::Object(object) => object
                .iter()
                .find(|(key, _)| key == "roundingMode")
                .map(|(_, value)| value),
            _ => None,
        });
    match value {
        None => Ok("halfExpand"),
        Some(Value::String(mode)) => Ok(mode.as_str()),
        _ => Err(crate::value::error::throw_type_error("Invalid roundingMode")),
    }
}

fn round_number(value: f64, mode: &str) -> f64 {
    match mode {
        "ceil" => value.ceil(),
        "floor" => value.floor(),
        "trunc" => value.trunc(),
        "expand" => value.signum() * value.abs().ceil(),
        _ => {
            let lower = value.floor();
            let fraction = value - lower;
            if fraction > 0.5 || (fraction == 0.5 && value.signum() >= 0.0) {
                lower + 1.0
            } else {
                lower
            }
        }
    }
}

fn duration_fields(object: &crate::value::ObjectData) -> Vec<Value> {
    [
        "years", "months", "weeks", "days", "hours", "minutes", "seconds",
        "milliseconds", "microseconds", "nanoseconds",
    ]
    .iter()
    .map(|name| {
        object
            .iter()
            .find(|(key, _)| key == name)
            .map_or(Value::Number(0.0), |(_, value)| value.clone())
    })
    .collect()
}

fn round_unit(value: Option<&Value>) -> Result<&str, VmError> {
    if value.is_none() || value.is_some_and(crate::conversion::is_symbol) {
        return Err(crate::value::error::throw_type_error("Options must be an object"));
    }
    match value {
        Some(Value::String(unit)) => Ok(unit.as_str()),
        Some(Value::Object(object)) => object
            .iter()
            .find(|(key, _)| key == "smallestUnit" || key == "largestUnit")
            .and_then(|(_, value)| match value {
                Value::String(unit) => Some(unit.as_str()),
                _ => None,
            })
            .ok_or_else(|| crate::value::error::throw_range_error("Invalid smallestUnit")),
        _ => Err(crate::value::error::throw_type_error("Options must be an object")),
    }
}

fn unit_index(unit: &str) -> Result<usize, VmError> {
    let unit = unit.strip_suffix('s').unwrap_or(unit);
    [
        ("year", 0), ("month", 1), ("week", 2), ("day", 3), ("hour", 4), ("minute", 5),
        ("second", 6), ("millisecond", 7), ("microsecond", 8),
        ("nanosecond", 9),
    ]
    .iter()
    .find(|(candidate, _)| *candidate == unit)
    .map(|(_, index)| *index)
    .ok_or_else(|| crate::value::error::throw_range_error("Invalid smallestUnit"))
}

fn calendar_round(
    object: &crate::value::ObjectData,
    relative: &Value,
    unit: usize,
) -> Result<Value, VmError> {
    let year = number_property(relative, "year") as i32;
    let month = number_property(relative, "month") as u32;
    let day = number_property(relative, "day") as u32;
    let start = NaiveDate::from_ymd_opt(year, month, day)
        .ok_or_else(|| crate::value::error::throw_range_error("Invalid relativeTo"))?;
    let total = duration_field(object, "years") as f64 * 365.0
        + duration_field(object, "months") as f64 * 30.0
        + duration_field(object, "weeks") as f64 * 7.0
        + duration_field(object, "days") as f64
        + duration_field(object, "hours") as f64 / 24.0;
    let sign = total.signum();
    let target = start
        .checked_add_signed(chrono::Duration::days(total as i64))
        .ok_or_else(|| crate::value::error::throw_range_error("Invalid relativeTo"))?;
    let mut cursor = start;
    let mut count = 0_i64;
    let limit = 10_000;
    for _ in 0..limit {
        let next = shift_calendar(cursor, unit, sign as i32)?;
        let reached = if sign >= 0.0 { next <= target } else { next >= target };
        if !reached {
            break;
        }
        cursor = next;
        count += sign as i64;
    }
    let next = shift_calendar(cursor, unit, sign as i32)?;
    let elapsed = if sign >= 0.0 {
        (cursor - start).num_days() as f64
    } else {
        (start - cursor).num_days() as f64
    };
    let remainder = (total.abs() - elapsed).max(0.0);
    let span = (next - cursor).num_days().abs() as f64;
    if span > 0.0 && remainder * 2.0 >= span {
        count += sign as i64;
    }
    let mut fields = vec![Value::Number(0.0); 10];
    if unit == 1 && duration_field(object, "years") != 0 {
        fields[0] = Value::Number((count / 12) as f64);
        fields[1] = Value::Number((count % 12) as f64);
    } else {
        fields[unit] = Value::Number(count as f64);
    }
    construct(&fields)
}

fn shift_calendar(date: NaiveDate, unit: usize, direction: i32) -> Result<NaiveDate, VmError> {
    let delta = if unit == 0 { 12 * direction } else { direction };
    let months = date.year() * 12 + date.month0() as i32 + delta;
    let year = months.div_euclid(12);
    let month = months.rem_euclid(12) as u32 + 1;
    let day = date.day().min(days_in_month(year, month));
    NaiveDate::from_ymd_opt(year, month, day)
        .ok_or_else(|| crate::value::error::throw_range_error("Invalid relativeTo"))
}

fn days_in_month(year: i32, month: u32) -> u32 {
    let next = if month == 12 {
        NaiveDate::from_ymd_opt(year + 1, 1, 1)
    } else {
        NaiveDate::from_ymd_opt(year, month + 1, 1)
    };
    next.map_or(31, |date| (date - chrono::Duration::days(1)).day())
}

fn largest_unit(value: Option<&Value>) -> Option<usize> {
    let Some(Value::Object(object)) = value else { return None };
    object
        .iter()
        .find(|(key, _)| key == "largestUnit")
        .and_then(|(_, value)| match value {
            Value::String(unit) => unit_index(unit).ok(),
            _ => None,
        })
}

fn balance_time_fields(fields: &mut [Value], first: usize) {
    for index in ((first + 1)..10).rev() {
        let base = match index {
            4 => 24,
            5 | 6 => 60,
            _ => 1_000,
        };
        let value = number_field(&fields[index]);
        let carry = value / base;
        fields[index] = Value::Number((value - carry * base) as f64);
        let next = number_field(&fields[index - 1]);
        fields[index - 1] = Value::Number((next + carry) as f64);
    }
}

fn balanced_sum(left: &crate::value::ObjectData, right: &crate::value::ObjectData) -> Vec<Value> {
    let years = sum_field(left, right, "years");
    let months = sum_field(left, right, "months");
    let weeks = sum_field(left, right, "weeks");
    let days = sum_field(left, right, "days");
    let time = [
        "hours",
        "minutes",
        "seconds",
        "milliseconds",
        "microseconds",
        "nanoseconds",
    ]
    .iter()
    .map(|name| sum_field(left, right, name))
    .collect::<Vec<_>>();
    let total = time[0] * 3_600_000_000_000
        + time[1] * 60_000_000_000
        + time[2] * 1_000_000_000
        + time[3] * 1_000_000
        + time[4] * 1_000
        + time[5];
    let sign = total.signum();
    let mut remainder = total.abs();
    let day_carry = remainder / 86_400_000_000_000;
    remainder %= 86_400_000_000_000;
    let hours = remainder / 3_600_000_000_000;
    remainder %= 3_600_000_000_000;
    let minutes = remainder / 60_000_000_000;
    remainder %= 60_000_000_000;
    let seconds = remainder / 1_000_000_000;
    remainder %= 1_000_000_000;
    [years, months, weeks, days + sign * day_carry]
        .into_iter()
        .chain(
            [
                hours,
                minutes,
                seconds,
                remainder / 1_000_000,
                remainder / 1_000 % 1_000,
                remainder % 1_000,
            ]
            .map(|value| value * sign),
        )
        .map(|value| Value::Number(value as f64))
        .collect()
}

fn sum_field(
    left: &crate::value::ObjectData,
    right: &crate::value::ObjectData,
    name: &str,
) -> i128 {
    duration_field(left, name) + duration_field(right, name)
}

fn to_json(receiver: Option<&Value>) -> Result<Value, VmError> {
    let object = duration_receiver(receiver)?;
    Ok(Value::String(format_iso_duration(object)))
}

fn format_iso_duration(object: &crate::value::ObjectData) -> String {
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
    let fields = names.map(|name| duration_field(object, name));
    let negative = fields.iter().any(|value| *value < 0);
    let fields = fields.map(|value| value.abs());
    let date = format_date_fields(&fields);
    let time = format_time_fields(&fields);
    let body = if date.is_empty() && time.is_empty() {
        "T0S".to_string()
    } else {
        format!("{date}{time}")
    };
    format!("{}P{body}", if negative { "-" } else { "" })
}

fn duration_field(object: &crate::value::ObjectData, name: &str) -> i128 {
    object
        .iter()
        .find(|(key, _)| key == name)
        .map_or(0, |(_, value)| number_field(&value))
}

fn number_field(value: &Value) -> i128 {
    match value {
        Value::Number(value) => *value as i128,
        _ => 0,
    }
}

fn format_date_fields(fields: &[i128; 10]) -> String {
    ["Y", "M", "W", "D"]
        .iter()
        .enumerate()
        .filter(|(index, _)| fields[*index] != 0)
        .map(|(index, suffix)| format!("{}{}", fields[index], suffix))
        .collect()
}

fn format_time_fields(fields: &[i128; 10]) -> String {
    let mut result = String::new();
    append_time_field(&mut result, fields[4], "H");
    append_time_field(&mut result, fields[5], "M");
    let subseconds = fields[7] * 1_000_000 + fields[8] * 1_000 + fields[9];
    let seconds = fields[6] + subseconds / 1_000_000_000;
    let remainder = subseconds % 1_000_000_000;
    if seconds != 0 || remainder != 0 {
        let fraction = format!("{remainder:09}").trim_end_matches('0').to_string();
        if fraction.is_empty() {
            append_time_field(&mut result, seconds, "S");
        } else {
            result.push_str(&format!("{seconds}.{fraction}S"));
        }
    }
    if result.is_empty() {
        String::new()
    } else {
        format!("T{result}")
    }
}

fn append_time_field(result: &mut String, value: i128, suffix: &str) {
    if value != 0 {
        result.push_str(&format!("{value}{suffix}"));
    }
}

fn duration_receiver(receiver: Option<&Value>) -> Result<&crate::value::ObjectData, VmError> {
    let Some(Value::Object(object)) = receiver else {
        return Err(crate::value::error::throw_type_error(
            "Temporal.Duration.prototype.abs called on incompatible receiver",
        ));
    };
    let branded = matches!(
        crate::builtins::object::is_prototype_of(
            Some(&Value::Builtin(
                crate::ops::Builtin::TemporalDurationPrototype
            )),
            &[Value::Object(object.clone())],
        )?,
        Value::Boolean(true)
    ) && has_duration_slots(object);
    branded.then_some(object.as_ref()).ok_or_else(|| {
        crate::value::error::throw_type_error(
            "Temporal.Duration.prototype.abs called on incompatible receiver",
        )
    })
}

pub(crate) fn validate_receiver(receiver: &Value) -> Result<(), VmError> {
    duration_receiver(Some(receiver)).map(|_| ())
}

fn has_duration_slots(object: &crate::value::ObjectData) -> bool {
    let fields = [
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
    fields.iter().all(|name| {
        object
            .iter()
            .any(|(key, value)| key == *name && matches!(value, Value::Number(_)))
    }) && object
        .iter()
        .any(|(key, value)| key == "sign" && matches!(value, Value::Number(_)))
        && object
            .iter()
            .any(|(key, value)| key == "blank" && matches!(value, Value::Boolean(_)))
}

fn absolute_fields(object: &crate::value::ObjectData) -> Vec<Value> {
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
    names
        .iter()
        .map(|name| {
            object
                .iter()
                .find(|(key, _)| key == name)
                .map_or(Value::Number(0.0), |(_, value)| match value {
                    Value::Number(value) => Value::Number(value.abs()),
                    _ => Value::Number(0.0),
                })
        })
        .collect()
}

include!("duration_helpers.rs");
