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
        crate::ops::Builtin::TemporalDuration => Some(Err(crate::value::error::throw_type_error(
            "Temporal.Duration constructor cannot be called without new",
        ))),
        crate::ops::Builtin::TemporalDurationFrom => Some(from(arguments.first())),
        crate::ops::Builtin::TemporalDurationCompare => Some(compare(arguments)),
        crate::ops::Builtin::TemporalDurationAdd => Some(add(receiver, arguments.first())),
        crate::ops::Builtin::TemporalDurationSubtract => {
            Some(subtract(receiver, arguments.first()))
        }
        crate::ops::Builtin::TemporalDurationWith => {
            Some(with_duration(receiver, arguments.first()))
        }
        crate::ops::Builtin::TemporalDurationAbs => Some(abs(receiver)),
        crate::ops::Builtin::TemporalDurationNegated => Some(negated(receiver)),
        crate::ops::Builtin::TemporalDurationRound => Some(round(receiver, arguments.first())),
        crate::ops::Builtin::TemporalDurationTotal => Some(total(receiver, arguments.first())),
        crate::ops::Builtin::TemporalDurationToLocaleString => Some(
            crate::intl::duration::format_temporal_duration(receiver, arguments),
        ),
        crate::ops::Builtin::TemporalDurationToJSON => Some(to_json(receiver)),
        crate::ops::Builtin::TemporalDurationToString => Some(to_string(receiver, arguments)),
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

fn total(receiver: Option<&Value>, options: Option<&Value>) -> Result<Value, VmError> {
    let object = duration_receiver(receiver)?;
    let unit = match options {
        Some(Value::String(unit)) => unit.clone(),
        Some(value) => match crate::execute::get_property_result(value, "unit")? {
            Value::String(unit) => unit,
            _ => return Err(crate::value::error::throw_range_error("Invalid unit")),
        },
        None => {
            return Err(crate::value::error::throw_type_error(
                "Options must be an object",
            ))
        }
    };
    let unit = unit.strip_suffix('s').unwrap_or(&unit);
    let factor = match unit {
        "day" => 86_400_000_000_000.0,
        "hour" => 3_600_000_000_000.0,
        "minute" => 60_000_000_000.0,
        "second" => 1_000_000_000.0,
        "millisecond" => 1_000_000.0,
        "microsecond" => 1_000.0,
        "nanosecond" => 1.0,
        _ => return Err(crate::value::error::throw_range_error("Invalid unit")),
    };
    if duration_field(object, "years") != 0 || duration_field(object, "months") != 0 {
        return Err(crate::value::error::throw_range_error(
            "relativeTo required",
        ));
    }
    let nanos = [
        ("weeks", 604_800_000_000_000_i128),
        ("days", 86_400_000_000_000),
        ("hours", 3_600_000_000_000),
        ("minutes", 60_000_000_000),
        ("seconds", 1_000_000_000),
        ("milliseconds", 1_000_000),
        ("microseconds", 1_000),
        ("nanoseconds", 1),
    ]
    .iter()
    .map(|(name, factor)| duration_field(object, name) as i128 * factor)
    .sum::<i128>();
    Ok(Value::Number(nanos as f64 / factor))
}

fn with_duration(receiver: Option<&Value>, options: Option<&Value>) -> Result<Value, VmError> {
    let object = duration_receiver(receiver)?;
    let Some(options) = options.filter(|value| crate::value::is_object(value)) else {
        return Err(crate::value::error::throw_type_error(
            "Duration.with requires an object",
        ));
    };
    let names = [
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
    let mut present = false;
    let canonical = [
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
    let mut fields = vec![Value::Number(0.0); canonical.len()];
    for name in names {
        let index = canonical
            .iter()
            .position(|candidate| candidate == &name)
            .unwrap();
        let value = {
            let value = crate::execute::get_property_result(options, name)?;
            if matches!(value, Value::Undefined) {
                Ok(Value::Number(duration_field(object, name) as f64))
            } else {
                present = true;
                crate::conversion::to_number(&value).map(Value::Number)
            }
        }?;
        fields[index] = value;
    }
    if !present {
        return Err(crate::value::error::throw_type_error(
            "Duration.with requires a duration field",
        ));
    }
    construct(&fields)
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
    reject_calendar_units(left, &right)?;
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
    reject_calendar_units(left, &negated)?;
    construct(&balanced_sum(left, &negated))
}

fn reject_calendar_units(
    left: &crate::value::ObjectData,
    right: &crate::value::ObjectData,
) -> Result<(), VmError> {
    if ["years", "months", "weeks"]
        .iter()
        .any(|name| duration_field(left, name) != 0 || duration_field(right, name) != 0)
    {
        return Err(crate::value::error::throw_range_error(
            "relativeTo required for calendar units",
        ));
    }
    Ok(())
}

fn negated(receiver: Option<&Value>) -> Result<Value, VmError> {
    let object = duration_receiver(receiver)?;
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
    ]
    .iter()
    .map(|name| Value::Number(-duration_field(object, name) as f64))
    .collect::<Vec<_>>();
    construct(&fields)
}

fn round(receiver: Option<&Value>, options: Option<&Value>) -> Result<Value, VmError> {
    let object = duration_receiver(receiver)?;
    let smallest = round_unit(options)?;
    let index = unit_index(&smallest)?;
    let has_calendar = ["years", "months", "weeks"]
        .iter()
        .any(|name| duration_field(object, name) != 0);
    let largest = largest_unit(options).unwrap_or_else(|| {
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
        .position(|name| number_field(&duration_field_value(object, name)) != 0)
        .unwrap_or(index)
        .min(if has_calendar { 9 } else { index })
    });
    let explicit_unit = options.is_some_and(|value| match value {
        Value::Object(object) => object.iter().any(|(key, value)| {
            matches!(key.as_str(), "smallestUnit" | "largestUnit")
                && !matches!(value, Value::Undefined)
        }),
        Value::String(_) => true,
        _ => false,
    });
    if !explicit_unit {
        return Err(crate::value::error::throw_range_error(
            "smallestUnit or largestUnit is required for calendar rounding",
        ));
    }
    if largest > index {
        return Err(crate::value::error::throw_range_error(
            "largestUnit must not be smaller than smallestUnit",
        ));
    }
    if index >= 4 && has_calendar {
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
        return calendar_time_round(object, &relative, options, index);
    }
    if index <= 3 {
        if index == 2
            && largest_unit(options).is_none()
            && duration_field(object, "months") != 0
            && rounding_increment(options, index)? > 1.0
        {
            return Err(crate::value::error::throw_range_error(
                "largestUnit is required for calendar rounding",
            ));
        }
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
        return calendar_round(
            object,
            &relative,
            options,
            index,
            largest_unit(options).is_none(),
        );
    }
    fixed_round(object, options, largest, index)
}

fn calendar_time_round(
    object: &crate::value::ObjectData,
    relative: &Value,
    options: Option<&Value>,
    index: usize,
) -> Result<Value, VmError> {
    let year = number_property(relative, "year") as i32;
    let month = number_property(relative, "month") as u32;
    let day = number_property(relative, "day") as u32;
    let start = NaiveDate::from_ymd_opt(year, month, day)
        .ok_or_else(|| crate::value::error::throw_range_error("Invalid relativeTo"))?;
    let sign = if duration_field(object, "years") < 0
        || duration_field(object, "months") < 0
        || duration_field(object, "weeks") < 0
        || duration_field(object, "days") < 0
    {
        -1.0
    } else {
        1.0
    };
    let mut target = shift_calendar_by(start, 0, duration_field(object, "years"))?;
    target = shift_calendar_by(target, 1, duration_field(object, "months"))?;
    target = shift_calendar_by(target, 2, duration_field(object, "weeks"))?;
    target = shift_calendar_by(target, 3, duration_field(object, "days"))?;
    let subday = duration_field(object, "hours") as f64 / 24.0
        + duration_field(object, "minutes") as f64 / 1_440.0
        + duration_field(object, "seconds") as f64 / 86_400.0
        + duration_field(object, "milliseconds") as f64 / 86_400_000.0
        + duration_field(object, "microseconds") as f64 / 86_400_000_000.0
        + duration_field(object, "nanoseconds") as f64 / 86_400_000_000_000.0;
    let mut cursor = start;
    let mut fields = vec![Value::Number(0.0); 10];
    let preserve = largest_unit(options).is_none();
    if preserve {
        for unit in 0..2 {
            let mut count = 0_i64;
            loop {
                let next = shift_calendar(cursor, unit, sign as i32)?;
                let reached = if sign >= 0.0 {
                    next <= target
                } else {
                    next >= target
                };
                if !reached {
                    break;
                }
                cursor = next;
                count += sign as i64;
            }
            fields[unit] = Value::Number(count as f64);
        }
    }
    let remaining_days = (target - cursor).num_days() as f64 + subday;
    let total_nanos = (remaining_days * 86_400_000_000_000.0) as i128;
    let scales = [
        86_400_000_000_000_i128,
        3_600_000_000_000,
        60_000_000_000,
        1_000_000_000,
        1_000_000,
        1_000,
        1,
    ];
    let increment = rounding_increment(options, index)? as i128;
    let rounded = round_integer(
        total_nanos,
        scales[index - 3] * increment,
        &rounding_mode(options)?,
    ) * scales[index - 3]
        * increment;
    let mut remainder = rounded.abs();
    let sign = rounded.signum();
    for unit in 3..=index {
        let value = remainder / scales[unit - 3];
        fields[unit] = Value::Number((value * sign) as f64);
        remainder %= scales[unit - 3];
    }
    construct(&fields)
}

fn duration_field_value(object: &crate::value::ObjectData, name: &str) -> Value {
    object
        .iter()
        .find(|(key, _)| key == name)
        .map_or(Value::Number(0.0), |(_, value)| value.clone())
}

fn fixed_round(
    object: &crate::value::ObjectData,
    options: Option<&Value>,
    largest: usize,
    index: usize,
) -> Result<Value, VmError> {
    let scales = [
        86_400_000_000_000_i128,
        3_600_000_000_000,
        60_000_000_000,
        1_000_000_000,
        1_000_000,
        1_000,
        1,
    ];
    let total = [
        "days",
        "hours",
        "minutes",
        "seconds",
        "milliseconds",
        "microseconds",
        "nanoseconds",
    ]
    .iter()
    .zip(scales)
    .map(|(name, scale)| duration_field(object, name) * scale)
    .sum::<i128>();
    let increment = rounding_increment(options, index)?;
    let quantum = scales[index - 3] * increment as i128;
    let mode = rounding_mode(options)?;
    let rounded_units = round_integer(total, quantum, &mode);
    let mut remainder = (rounded_units * quantum).abs();
    let mut rounded = vec![Value::Number(0.0); 10];
    let sign = rounded_units.signum();
    for unit in largest.max(3)..=index {
        let component = remainder / scales[unit - 3];
        rounded[unit] = Value::Number((component * sign) as f64);
        remainder %= scales[unit - 3];
    }
    if largest == 2 {
        let days = rounded[3].as_number().unwrap_or(0.0) as i128;
        rounded[2] = Value::Number((days / 7) as f64);
        rounded[3] = Value::Number((days % 7) as f64);
    }
    construct(&rounded)
}

fn round_integer(value: i128, quantum: i128, mode: &str) -> i128 {
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
    units * sign
}

fn rounding_increment(options: Option<&Value>, index: usize) -> Result<f64, VmError> {
    let value = options
        .and_then(|value| match value {
            Value::Object(object) => object
                .iter()
                .find(|(key, _)| key == "roundingIncrement")
                .map(|(_, value)| value),
            _ => None,
        })
        .filter(|value| !matches!(value, Value::Undefined))
        .map(|value| crate::conversion::to_number(&value))
        .transpose()?
        .unwrap_or(1.0);
    if !value.is_finite() || value <= 0.0 || value.fract() != 0.0 {
        return Err(crate::value::error::throw_range_error(
            "Invalid roundingIncrement",
        ));
    }
    if index >= 4 {
        let maximum = [24.0, 60.0, 60.0, 1_000.0, 1_000.0, 1_000.0][index - 4];
        if value >= maximum || (maximum % value) != 0.0 {
            return Err(crate::value::error::throw_range_error(
                "Invalid roundingIncrement",
            ));
        }
    }
    Ok(value)
}

fn rounding_mode(options: Option<&Value>) -> Result<String, VmError> {
    let value = options.and_then(|value| match value {
        Value::Object(object) => object
            .iter()
            .find(|(key, _)| key == "roundingMode")
            .map(|(_, value)| value),
        _ => None,
    });
    match value {
        None | Some(Value::Undefined) => Ok("halfExpand".into()),
        Some(Value::String(mode)) => Ok(mode.clone()),
        _ => Err(crate::value::error::throw_type_error(
            "Invalid roundingMode",
        )),
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
    .map(|name| {
        object
            .iter()
            .find(|(key, _)| key == name)
            .map_or(Value::Number(0.0), |(_, value)| value.clone())
    })
    .collect()
}

fn round_unit(value: Option<&Value>) -> Result<String, VmError> {
    if value.is_none() || value.is_some_and(crate::conversion::is_symbol) {
        return Err(crate::value::error::throw_type_error(
            "Options must be an object",
        ));
    }
    match value {
        Some(Value::String(unit)) => Ok(unit.clone()),
        Some(Value::Object(object)) => match object.iter().find(|(key, _)| key == "smallestUnit") {
            Some((_, Value::String(unit))) => Ok(unit.clone()),
            Some((_, Value::Undefined)) | None => object
                .iter()
                .find(|(key, _)| key == "largestUnit")
                .and_then(|(_, value)| match value {
                    Value::String(unit) if unit_index(&unit).is_ok() => Some(unit.clone()),
                    _ => None,
                })
                .map_or_else(|| Ok("nanosecond".into()), Ok),
            Some(_) => Err(crate::value::error::throw_range_error(
                "Invalid smallestUnit",
            )),
        },
        _ => Err(crate::value::error::throw_type_error(
            "Options must be an object",
        )),
    }
}

fn unit_index(unit: &str) -> Result<usize, VmError> {
    let unit = unit.strip_suffix('s').unwrap_or(unit);
    [
        ("year", 0),
        ("month", 1),
        ("week", 2),
        ("day", 3),
        ("hour", 4),
        ("minute", 5),
        ("second", 6),
        ("millisecond", 7),
        ("microsecond", 8),
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
    options: Option<&Value>,
    unit: usize,
    preserve_larger: bool,
) -> Result<Value, VmError> {
    let year = number_property(relative, "year") as i32;
    let month = number_property(relative, "month") as u32;
    let day = number_property(relative, "day") as u32;
    let start = NaiveDate::from_ymd_opt(year, month, day)
        .ok_or_else(|| crate::value::error::throw_range_error("Invalid relativeTo"))?;
    let sign = if duration_field(object, "years") < 0
        || duration_field(object, "months") < 0
        || duration_field(object, "weeks") < 0
        || duration_field(object, "days") < 0
        || duration_field(object, "hours") < 0
    {
        -1.0
    } else {
        1.0
    };
    let mut target = shift_calendar_by(start, 0, duration_field(object, "years"))?;
    target = shift_calendar_by(target, 1, duration_field(object, "months"))?;
    target = shift_calendar_by(target, 2, duration_field(object, "weeks"))?;
    target = shift_calendar_by(target, 3, duration_field(object, "days"))?;
    let subday = duration_field(object, "hours") as f64 / 24.0
        + duration_field(object, "minutes") as f64 / 1_440.0
        + duration_field(object, "seconds") as f64 / 86_400.0
        + duration_field(object, "milliseconds") as f64 / 86_400_000.0
        + duration_field(object, "microseconds") as f64 / 86_400_000_000.0
        + duration_field(object, "nanoseconds") as f64 / 86_400_000_000_000.0;
    let whole_subday = subday.trunc() as i64;
    target = target
        .checked_add_signed(chrono::Duration::days(whole_subday))
        .ok_or_else(|| crate::value::error::throw_range_error("Invalid relativeTo"))?;
    let target_distance = (target - start).num_days() as f64 + subday.fract();
    let total = target_distance * sign;
    let mut cursor = start;
    if preserve_larger {
        for larger_unit in 0..unit {
            if larger_unit == 2 && unit >= 3 {
                continue;
            }
            loop {
                let next = shift_calendar(cursor, larger_unit, sign as i32)?;
                let reached = if sign >= 0.0 {
                    next <= target
                } else {
                    next >= target
                };
                if !reached {
                    break;
                }
                cursor = next;
            }
        }
    }
    let mut count = 0_i64;
    let limit = 10_000;
    for _ in 0..limit {
        let next = shift_calendar(cursor, unit, sign as i32)?;
        let reached = if sign >= 0.0 {
            next <= target
        } else {
            next >= target
        };
        if !reached {
            break;
        }
        cursor = next;
        count += sign as i64;
    }
    let next = shift_calendar(cursor, unit, sign as i32)?;
    let elapsed = (cursor - start).num_days().unsigned_abs() as f64;
    let remainder = (total.abs() - elapsed).max(0.0);
    let span = (next - cursor).num_days().abs() as f64;
    let unrounded_count = count;
    if span > 0.0 && remainder * 2.0 >= span {
        count += sign as i64;
    }
    let increment = rounding_increment(options, unit)? as i128;
    count = (round_integer(count as i128, increment, &rounding_mode(options)?) * increment) as i64;
    let mut fields = vec![Value::Number(0.0); 10];
    if preserve_larger {
        let mut larger_cursor = start;
        for index in 0..unit {
            if index == 2 && unit >= 3 {
                continue;
            }
            let mut larger_count = 0_i64;
            loop {
                let next = shift_calendar(larger_cursor, index, sign as i32)?;
                let reached = if sign >= 0.0 {
                    next <= target
                } else {
                    next >= target
                };
                if !reached {
                    break;
                }
                larger_cursor = next;
                larger_count += sign as i64;
            }
            fields[index] = Value::Number(larger_count as f64);
        }
    }
    if unit == 1 && preserve_larger {
        fields[1] = Value::Number(count as f64);
    } else {
        fields[unit] = Value::Number(count as f64);
    }
    if count == unrounded_count {
        let mut lower_cursor = cursor;
        for lower_unit in (unit + 1)..=3 {
            if lower_unit == 2 && unit < 2 {
                continue;
            }
            let mut lower_count = 0_i64;
            loop {
                let next = shift_calendar(lower_cursor, lower_unit, sign as i32)?;
                let reached = if sign >= 0.0 {
                    next <= target
                } else {
                    next >= target
                };
                if !reached {
                    break;
                }
                lower_cursor = next;
                lower_count += sign as i64;
            }
            fields[lower_unit] = Value::Number(lower_count as f64);
        }
    }
    construct(&fields)
}

fn shift_calendar(date: NaiveDate, unit: usize, direction: i32) -> Result<NaiveDate, VmError> {
    if unit == 2 {
        return date
            .checked_add_signed(chrono::Duration::days(i64::from(direction) * 7))
            .ok_or_else(|| crate::value::error::throw_range_error("Invalid relativeTo"));
    }
    if unit == 3 {
        return date
            .checked_add_signed(chrono::Duration::days(i64::from(direction)))
            .ok_or_else(|| crate::value::error::throw_range_error("Invalid relativeTo"));
    }
    let delta = if unit == 0 { 12 * direction } else { direction };
    let months = date.year() * 12 + date.month0() as i32 + delta;
    let year = months.div_euclid(12);
    let month = months.rem_euclid(12) as u32 + 1;
    let day = date.day().min(days_in_month(year, month));
    NaiveDate::from_ymd_opt(year, month, day)
        .ok_or_else(|| crate::value::error::throw_range_error("Invalid relativeTo"))
}

fn shift_calendar_by(date: NaiveDate, unit: usize, amount: i128) -> Result<NaiveDate, VmError> {
    if unit >= 2 {
        let days = if unit == 2 { amount * 7 } else { amount };
        return date
            .checked_add_signed(chrono::Duration::days(days as i64))
            .ok_or_else(|| crate::value::error::throw_range_error("Invalid relativeTo"));
    }
    let delta = amount * if unit == 0 { 12 } else { 1 };
    let months = i128::from(date.year()) * 12 + i128::from(date.month0()) + delta;
    let year = months.div_euclid(12);
    let month = months.rem_euclid(12) as u32 + 1;
    let day = date.day().min(days_in_month(year as i32, month));
    NaiveDate::from_ymd_opt(year as i32, month, day)
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
    let Some(Value::Object(object)) = value else {
        return None;
    };
    object
        .iter()
        .find(|(key, _)| key == "largestUnit")
        .and_then(|(_, value)| match value {
            Value::String(unit) => unit_index(&unit).ok(),
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
    let days = sum_field(left, right, "days");
    let time_names = [
        "hours",
        "minutes",
        "seconds",
        "milliseconds",
        "microseconds",
        "nanoseconds",
    ];
    let time = time_names
        .iter()
        .map(|name| sum_field(left, right, name))
        .collect::<Vec<_>>();
    let scales = [
        3_600_000_000_000_i128,
        60_000_000_000,
        1_000_000_000,
        1_000_000,
        1_000,
        1,
    ];
    let total = time
        .iter()
        .zip(scales)
        .map(|(value, scale)| value * scale)
        .sum::<i128>();
    let sign = total.signum();
    let largest = if days != 0 {
        0
    } else {
        time.iter().position(|value| *value != 0).unwrap_or(5)
    };
    let mut remainder = total.abs();
    let mut fields = vec![0_i128; 10];
    if days != 0 {
        fields[3] = days + sign * (remainder / 86_400_000_000_000);
        remainder %= 86_400_000_000_000;
    }
    for index in largest..6 {
        fields[index + 4] = remainder / scales[index];
        remainder %= scales[index];
    }
    fields
        .into_iter()
        .enumerate()
        .map(|(index, value)| {
            let value = if index >= 4 { value * sign } else { value };
            Value::Number(value as f64)
        })
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

fn to_string(receiver: Option<&Value>, arguments: &[Value]) -> Result<Value, VmError> {
    let object = duration_receiver(receiver)?;
    let options = match arguments.first() {
        None | Some(Value::Undefined) => None,
        Some(value) if crate::value::is_object(value) => Some(value),
        _ => {
            return Err(crate::value::error::throw_type_error(
                "Options must be an object",
            ))
        }
    };
    let digits = if let Some(options) = options {
        let smallest = crate::execute::get_property_result(options, "smallestUnit")?;
        if !matches!(smallest, Value::Undefined) {
            Some(match crate::conversion::to_string(&smallest)?.as_str() {
                "second" | "seconds" => 0,
                "millisecond" | "milliseconds" => 3,
                "microsecond" | "microseconds" => 6,
                "nanosecond" | "nanoseconds" => 9,
                _ => {
                    return Err(crate::value::error::throw_range_error(
                        "Invalid smallestUnit",
                    ))
                }
            })
        } else {
            let value = crate::execute::get_property_result(options, "fractionalSecondDigits")?;
            if matches!(value, Value::String(ref value) if value == "auto") {
                return Ok(Value::String(format_iso_duration_with_digits(object, None)));
            }
            if matches!(value, Value::Undefined) {
                None
            } else {
                let number = crate::conversion::to_number(&value)?;
                if !number.is_finite() || number < -0.0 || number > 9.0 {
                    return Err(crate::value::error::throw_range_error(
                        "Invalid fractionalSecondDigits",
                    ));
                }
                Some(number.floor() as usize)
            }
        }
    } else {
        None
    };
    Ok(Value::String(format_iso_duration_with_digits(
        object, digits,
    )))
}

fn format_iso_duration(object: &crate::value::ObjectData) -> String {
    format_iso_duration_with_digits(object, None)
}

fn format_iso_duration_with_digits(
    object: &crate::value::ObjectData,
    digits: Option<usize>,
) -> String {
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
    let time = format_time_fields(&fields, digits);
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

fn format_time_fields(fields: &[i128; 10], digits: Option<usize>) -> String {
    let mut result = String::new();
    append_time_field(&mut result, fields[4], "H");
    append_time_field(&mut result, fields[5], "M");
    let subseconds = fields[7] * 1_000_000 + fields[8] * 1_000 + fields[9];
    let seconds = fields[6] + subseconds / 1_000_000_000;
    let remainder = subseconds % 1_000_000_000;
    if seconds != 0 || remainder != 0 {
        let fraction = match digits {
            Some(0) => String::new(),
            Some(digits) => format!("{:09}", remainder)[..digits].to_string(),
            None => format!("{remainder:09}").trim_end_matches('0').to_string(),
        };
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
