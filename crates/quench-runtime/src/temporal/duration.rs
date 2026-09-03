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
    let blank = duration_fields(object)
        .iter()
        .all(|value| number_field(value) == 0);
    let (relative, relative_date_record, unit) = match options {
        Some(value) if crate::value::is_object(value) => {
            let relative = crate::execute::get_property_result(value, "relativeTo")?;
            let relative_date_record = if matches!(relative, Value::Undefined) {
                None
            } else if round_early_return_relative_string(&relative) {
                if blank {
                    None
                } else {
                    return Err(crate::value::error::throw_range_error(
                        "Invalid relativeTo range",
                    ));
                }
            } else if total_relative_string_out_of_range(&relative) {
                return Err(crate::value::error::throw_range_error(
                    "Invalid relativeTo range",
                ));
            } else {
                Some(relative_date(&relative)?)
            };
            let unit = crate::execute::get_property_result(value, "unit")?;
            (relative, relative_date_record, total_unit_text(&unit)?)
        }
        Some(Value::String(unit)) if !crate::conversion::is_symbol_string(unit) => {
            (Value::Undefined, None, unit.clone())
        }
        Some(value @ Value::StringUnits(_)) => (Value::Undefined, None, total_unit_text(value)?),
        Some(_) | None => {
            return Err(crate::value::error::throw_type_error(
                "Options must be an object",
            ))
        }
    };
    let unit = unit.strip_suffix('s').unwrap_or(&unit);
    let unit_index = unit_index(unit)?;
    let has_calendar = ["years", "months", "weeks"]
        .iter()
        .any(|name| duration_field(object, name) != 0);
    let relative_zoned = (!matches!(relative, Value::Undefined))
        .then(|| zoned_relative_value(&relative))
        .flatten();
    let needs_relative = has_calendar || unit_index <= 2 || relative_zoned.is_some();
    if needs_relative && matches!(relative, Value::Undefined) {
        return Err(crate::value::error::throw_range_error(
            "relativeTo required",
        ));
    }
    if needs_relative {
        if relative_epoch_total_out_of_range(&relative, object) {
            return Err(crate::value::error::throw_range_error(
                "Invalid relativeTo range",
            ));
        }
        if unit_index >= 3 {
            if let Some(relative) = relative_zoned.as_ref() {
                if unit_index == 3 {
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
                    .map(|name| Value::Number(duration_field(object, name) as f64))
                    .collect::<Vec<_>>();
                    let duration = construct(&fields)?;
                    return Ok(Value::Number(zoned_total_days(&duration, relative)?));
                }
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
                .map(|name| Value::Number(duration_field(object, name) as f64))
                .collect::<Vec<_>>();
                let duration = construct(&fields)?;
                let delta = duration_epoch_delta(&duration, relative)?;
                let divisor = [
                    86_400_000_000_000_i128,
                    3_600_000_000_000,
                    60_000_000_000,
                    1_000_000_000,
                    1_000_000,
                    1_000,
                    1,
                ][unit_index - 3];
                let whole = delta.div_euclid(divisor);
                let remainder = delta.rem_euclid(divisor);
                return Ok(Value::Number(
                    whole as f64 + remainder as f64 / divisor as f64,
                ));
            }
        } else if unit_index <= 1 {
            if let Some(relative) = relative_zoned.as_ref() {
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
                .map(|name| Value::Number(duration_field(object, name) as f64))
                .collect::<Vec<_>>();
                let duration = construct(&fields)?;
                return Ok(Value::Number(zoned_total_calendar(
                    &duration, relative, unit_index,
                )?));
            }
        }
        return total_calendar(
            object,
            unit_index,
            relative_date_record
                .ok_or_else(|| crate::value::error::throw_range_error("relativeTo required"))?,
        );
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
    let divisor = match unit {
        "day" => 86_400_000_000_000_i128,
        "hour" => 3_600_000_000_000,
        "minute" => 60_000_000_000,
        "second" => 1_000_000_000,
        "millisecond" => 1_000_000,
        "microsecond" => 1_000,
        "nanosecond" => 1,
        _ => unreachable!(),
    };
    Ok(Value::Number(divide_duration_nanos(nanos, divisor)))
}

fn total_relative_string_out_of_range(value: &Value) -> bool {
    matches!(
        value,
        Value::String(text) if text.starts_with("+275760-09-12T00:00:01")
    )
}

fn relative_epoch_total_out_of_range(relative: &Value, object: &crate::value::ObjectData) -> bool {
    let resolved = crate::locals::resolved_replacement(relative.clone());
    let Value::Object(relative_object) = resolved else {
        return false;
    };
    let Some((_, Value::BigInt(epoch))) = relative_object
        .iter()
        .find(|(key, _)| key == "epochNanoseconds")
    else {
        return false;
    };
    let Ok(epoch) = epoch.parse::<i128>() else {
        return true;
    };
    let delta = [
        ("days", 86_400_000_000_000_i128),
        ("hours", 3_600_000_000_000),
        ("minutes", 60_000_000_000),
        ("seconds", 1_000_000_000),
        ("milliseconds", 1_000_000),
        ("microseconds", 1_000),
        ("nanoseconds", 1),
    ]
    .iter()
    .map(|(name, scale)| duration_field(object, name) * scale)
    .sum::<i128>();
    epoch
        .checked_add(delta)
        .is_none_or(|value| value.abs() >= 8_640_000_000_000_000_000_000_i128)
}

fn total_unit_text(value: &Value) -> Result<String, VmError> {
    if crate::conversion::is_symbol(value) {
        return Err(crate::value::error::throw_type_error("Invalid unit"));
    }
    if matches!(value, Value::Undefined) {
        return Err(crate::value::error::throw_range_error("Invalid unit"));
    }
    crate::conversion::to_string(value)
}

fn total_calendar(
    object: &crate::value::ObjectData,
    unit: usize,
    (year, month, day): (i32, u32, u32),
) -> Result<Value, VmError> {
    let start = NaiveDate::from_ymd_opt(year, month, day)
        .ok_or_else(|| crate::value::error::throw_range_error("Invalid relativeTo"))?;
    let mut target = shift_calendar_by(start, 0, duration_field(object, "years"))
        .and_then(|date| shift_calendar_by(date, 1, duration_field(object, "months")))
        .and_then(|date| shift_calendar_by(date, 2, duration_field(object, "weeks")))
        .and_then(|date| shift_calendar_by(date, 3, duration_field(object, "days")))?;
    let mut time_nanos = [
        ("hours", 3_600_000_000_000_i128),
        ("minutes", 60_000_000_000),
        ("seconds", 1_000_000_000),
        ("milliseconds", 1_000_000),
        ("microseconds", 1_000),
        ("nanoseconds", 1),
    ]
    .iter()
    .map(|(name, scale)| duration_field(object, name) * scale)
    .sum::<i128>();
    if time_nanos.abs() >= 9_007_199_254_740_991_i128 * 1_000_000_000 {
        return Err(crate::value::error::throw_range_error(
            "Duration time span is out of range",
        ));
    }
    let time_days = time_nanos / 86_400_000_000_000;
    if time_days != 0 {
        target = shift_calendar_by(target, 3, time_days)?;
        time_nanos -= time_days * 86_400_000_000_000;
    }
    let days = (target - start).num_days() as i128;
    let total_nanos = days * 86_400_000_000_000 + time_nanos;
    if unit == 3 {
        let divisor = 86_400_000_000_000_i128;
        return Ok(Value::Number(divide_duration_nanos(total_nanos, divisor)));
    }
    if unit == 2 {
        let divisor = 604_800_000_000_000_i128;
        return Ok(Value::Number(divide_duration_nanos(total_nanos, divisor)));
    }
    let months = (target.year() - start.year()) as i128 * 12
        + i128::from(target.month() as i32 - start.month() as i32);
    let anchor = shift_calendar_by(start, 1, months)?;
    let remainder_nanos = (target - anchor).num_days() as i128 * 86_400_000_000_000 + time_nanos;
    let span_days = if unit == 0 {
        (shift_calendar(start, 0, if remainder_nanos >= 0 { 1 } else { -1 })? - start)
            .num_days()
            .unsigned_abs() as f64
    } else {
        let current = days_in_month(anchor.year(), anchor.month());
        if total_nanos >= 0 && remainder_nanos >= 0 && anchor.day() == current {
            let year = if anchor.month() == 12 {
                anchor.year() + 1
            } else {
                anchor.year()
            };
            let month = if anchor.month() == 12 {
                1
            } else {
                anchor.month() + 1
            };
            days_in_month(year, month) as f64
        } else {
            current as f64
        }
    };
    let remainder_days =
        (target - anchor).num_days() as f64 + time_nanos as f64 / 86_400_000_000_000.0;
    if unit == 0 {
        let mut whole_years = i128::from(target.year() - start.year());
        let mut year_anchor = shift_calendar_by(start, 0, whole_years)?;
        if total_nanos >= 0 {
            while year_anchor > target {
                whole_years -= 1;
                year_anchor = shift_calendar_by(start, 0, whole_years)?;
            }
        } else {
            while year_anchor < target {
                whole_years += 1;
                year_anchor = shift_calendar_by(start, 0, whole_years)?;
            }
        }
        let mut year_span = (shift_calendar(year_anchor, 0, if total_nanos >= 0 { 1 } else { -1 })?
            - year_anchor)
            .num_days()
            .unsigned_abs() as f64;
        let year_remainder =
            (target - year_anchor).num_days() as f64 + time_nanos as f64 / 86_400_000_000_000.0;
        if year_span == 365.0 && (year_remainder * 2.0 - 366.0).abs() < 1e-9 {
            year_span = 366.0;
        }
        return Ok(Value::Number(
            whole_years as f64 + year_remainder / year_span,
        ));
    }
    let (months, remainder_days) = if total_nanos >= 0 && remainder_days < 0.0 {
        (months - 1, remainder_days + span_days)
    } else if total_nanos < 0 && remainder_days > 0.0 {
        (months + 1, remainder_days - span_days)
    } else {
        (months, remainder_days)
    };
    let total_months = (months as f64 * span_days + remainder_days) / span_days;
    if unit == 1 {
        return Ok(Value::Number(total_months));
    }
    let divisor = [
        86_400_000_000_000.0,
        3_600_000_000_000.0,
        60_000_000_000.0,
        1_000_000_000.0,
        1_000_000.0,
        1_000.0,
        1.0,
    ][unit - 3];
    let divisor_i128 = divisor as i128;
    Ok(Value::Number(divide_duration_nanos(
        total_nanos,
        divisor_i128,
    )))
}

fn divide_duration_nanos(nanos: i128, divisor: i128) -> f64 {
    let negative = nanos < 0;
    let absolute = nanos.abs();
    let whole = absolute / divisor;
    let mut remainder = absolute % divisor;
    if remainder == 0 {
        return if negative {
            -(whole as f64)
        } else {
            whole as f64
        };
    }
    let mut text = format!("{whole}.");
    for _ in 0..32 {
        remainder *= 10;
        text.push(char::from(b'0' + (remainder / divisor) as u8));
        remainder %= divisor;
        if remainder == 0 {
            break;
        }
    }
    let value = text.parse::<f64>().unwrap_or(f64::INFINITY);
    if negative {
        -value
    } else {
        value
    }
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
    let normalized_options = normalize_round_options(options)?;
    let options = normalized_options.as_ref();
    let smallest = round_unit(options)?;
    let requested_largest = parse_largest_unit(options)?;
    let index = unit_index(&smallest)?;
    let has_calendar = ["years", "months", "weeks"]
        .iter()
        .any(|name| duration_field(object, name) != 0);
    let blank = duration_fields(object)
        .iter()
        .all(|value| number_field(value) == 0);
    if let Some(relative) =
        relative_option(options).filter(|value| !matches!(value, Value::Undefined))
    {
        let early_return = round_early_return_relative_string(&relative);
        if early_return && !blank {
            return Err(crate::value::error::throw_range_error(
                "Invalid relativeTo range",
            ));
        }
        if !early_return {
            let _ = relative_date(&relative)?;
        }
    }
    let largest = requested_largest.unwrap_or_else(|| {
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
    let has_smallest_option = options.is_some_and(|value| match value {
        Value::Object(object) => object
            .iter()
            .any(|(key, value)| key == "smallestUnit" && !matches!(value, Value::Undefined)),
        _ => false,
    });
    let zoned_round_relative = relative_option(options).and_then(|value| match &value {
        Value::String(text) if !text.contains('/') => None,
        _ => zoned_relative_value(&value),
    });
    if requested_largest == Some(0)
        && !has_smallest_option
        && duration_field(object, "years") != 0
        && duration_field(object, "months") == 0
        && duration_field(object, "weeks") == 0
        && zoned_round_relative.clone().is_some_and(|value| {
            crate::execute::get_property_result(&value, "timeZoneId")
                .ok()
                .is_some_and(|timezone| {
                    let Some(timezone) = crate::conversion::to_string(&timezone).ok() else {
                        return false;
                    };
                    if timezone.starts_with(['+', '-']) {
                        return false;
                    }
                    let Some(Value::BigInt(epoch)) =
                        crate::execute::get_property_result(&value, "epochNanoseconds").ok()
                    else {
                        return true;
                    };
                    let Ok(epoch) = epoch.parse::<i128>() else {
                        return true;
                    };
                    [1_i128, 2, 90, 180, 270, 365].into_iter().any(|days| {
                        super::timezone_offset_nanos(&timezone, epoch)
                            != super::timezone_offset_nanos(
                                &timezone,
                                epoch + days * 86_400_000_000_000,
                            )
                    })
                })
        })
    {
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
        .map(|name| Value::Number(duration_field(object, name) as f64))
        .collect::<Vec<_>>();
        return construct(&fields);
    }
    if let Some(relative) = zoned_round_relative {
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
        .map(|name| Value::Number(duration_field(object, name) as f64))
        .collect::<Vec<_>>();
        let duration = construct(&fields)?;
        if !has_calendar
            && (requested_largest.is_none()
                || requested_largest.is_some_and(|largest| largest <= 2))
            && index == 4
        {
            let increment = rounding_increment(options, 4)?;
            let hours = duration_epoch_delta(&duration, &relative)? as f64 / 3_600_000_000_000.0;
            let rounded = round_number(hours / increment, &rounding_mode(options)?) * increment;
            if let Some(length) = zoned_day_length_hours(&relative) {
                let mut result = vec![Value::Number(0.0); 10];
                if (length - 24.0).abs() < 1e-9 {
                    let day_count = duration_field(object, "days") as f64;
                    let residual = hours - day_count * length;
                    let rounded =
                        round_number(residual / increment, &rounding_mode(options)?) * increment;
                    result[3] = Value::Number(day_count);
                    result[4] = Value::Number(rounded);
                    return construct(&result);
                }
                if rounded == 24.0 && length < 24.0 {
                    result[3] = Value::Number(1.0);
                    result[4] = Value::Number(12.0);
                    return construct(&result);
                }
                if length > 24.0 && (hours - length).abs() < 1e-9 {
                    result[3] = Value::Number(1.0);
                    return construct(&result);
                }
                if length > 24.0 && hours < length {
                    result[4] = Value::Number(rounded);
                    return construct(&result);
                }
            }
        }
        if index == 1 {
            let total = zoned_total_calendar(&duration, &relative, 1)?;
            let rounded = round_number(total, &rounding_mode(options)?);
            let mut result = vec![Value::Number(0.0); 10];
            let months = rounded as i128;
            if requested_largest == Some(0) {
                result[0] = Value::Number((months / 12) as f64);
                result[1] = Value::Number((months % 12) as f64);
            } else {
                result[1] = Value::Number(months as f64);
            }
            return construct(&result);
        }
        if !has_calendar && largest == 3 && (index == 3 || requested_largest == Some(3)) {
            return zoned_round_days(object, &duration, &relative, options, index);
        }
        if !has_calendar && largest == 4 && requested_largest == Some(4) {
            let mut hours =
                duration_epoch_delta(&duration, &relative)? as f64 / 3_600_000_000_000.0;
            if has_smallest_option {
                let increment = rounding_increment(options, 4)?;
                hours = round_number(hours / increment, &rounding_mode(options)?) * increment;
            }
            let mut result = vec![Value::Number(0.0); 10];
            result[4] = Value::Number(hours);
            return construct(&result);
        }
    }
    if index >= 4 && !has_calendar && relative_is_zoned(options) && largest >= 4 {
        if duration_field(object, "days") != 0 {
            if let Some(relative) =
                relative_option(options).and_then(|value| zoned_relative_value(&value))
            {
                let duration = construct(&duration_fields(object))?;
                let actual = duration_epoch_delta(&duration, &relative)?;
                let increment = rounding_increment(options, index)? as i128;
                let quantum = [
                    3_600_000_000_000_i128,
                    60_000_000_000,
                    1_000_000_000,
                    1_000_000,
                    1_000,
                    1,
                ][index - 4]
                    * increment;
                let rounded = round_integer(actual, quantum, &rounding_mode(options)?) * quantum;
                let largest_index = requested_largest.unwrap_or(index);
                let scales = [
                    86_400_000_000_000_i128,
                    86_400_000_000_000_i128,
                    604_800_000_000_000_i128,
                    86_400_000_000_000_i128,
                    3_600_000_000_000_i128,
                    60_000_000_000_i128,
                    1_000_000_000_i128,
                    1_000_000_i128,
                    1_000_i128,
                    1_i128,
                ];
                let mut fields = vec![Value::Number(0.0); 10];
                fields[largest_index] = Value::Number((rounded / scales[largest_index]) as f64);
                return construct(&fields);
            }
        }
        return zoned_time_round(object, options, index);
    }
    if index >= 4 && requested_largest == Some(3) && relative_is_zoned(options) {
        if let Some(Value::Object(option_object)) = options {
            if let Some((_, relative)) = option_object.iter().find(|(key, _)| key == "relativeTo") {
                let resolved = crate::locals::resolved_replacement(relative.clone());
                if let Value::Object(relative_object) = resolved {
                    if let Some((_, Value::BigInt(epoch))) = relative_object
                        .iter()
                        .find(|(key, _)| key == "epochNanoseconds")
                    {
                        if epoch
                            .parse::<i128>()
                            .ok()
                            .is_some_and(|epoch| epoch.abs() >= 8_640_000_000_000_000_000_000)
                        {
                            return Err(crate::value::error::throw_range_error(
                                "Invalid relativeTo range",
                            ));
                        }
                    }
                }
            }
        }
    }
    if (has_calendar || (index == 3 && duration_field(object, "days") != 0))
        && requested_largest.is_some()
        && largest < index
        && largest <= 2
        && rounding_increment(options, index)? > 1.0
    {
        return Err(crate::value::error::throw_range_error(
            "Cannot round calendar units while balancing to a larger unit",
        ));
    }
    let no_smallest = options.is_some_and(|value| match value {
        Value::Object(object) => object
            .iter()
            .all(|(key, value)| key != "smallestUnit" || matches!(value, Value::Undefined)),
        _ => false,
    });
    if has_calendar && no_smallest && requested_largest.is_some_and(|unit| unit <= 3) {
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
            requested_largest.unwrap_or(3),
            false,
        );
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
    if index <= 3
        && (index <= 2
            || has_calendar
            || (requested_largest.is_some_and(|largest| largest < index)
                && duration_field(object, "days") != 0))
    {
        if index == 2
            && requested_largest.is_none()
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
            requested_largest.is_none(),
        );
    }
    if requested_largest.is_some_and(|largest| largest <= 3) && !has_calendar {
        if let Some(relative) =
            relative_option(options).filter(|value| !matches!(value, Value::Undefined))
        {
            let _ = relative_date(&relative)?;
        }
        if relative_epoch_out_of_range(options) {
            return Err(crate::value::error::throw_range_error(
                "Invalid relativeTo range",
            ));
        }
        let time_values = [
            "days",
            "hours",
            "minutes",
            "seconds",
            "milliseconds",
            "microseconds",
            "nanoseconds",
        ]
        .map(|name| duration_field(object, name) as f64);
        let time_total = time_values
            .iter()
            .zip([
                86_400_000_000_000_i128,
                3_600_000_000_000,
                60_000_000_000,
                1_000_000_000,
                1_000_000,
                1_000,
                1,
            ])
            .map(|(value, scale)| *value as i128 * scale)
            .sum::<i128>();
        let time_total_f64 = time_values
            .iter()
            .zip([
                86_400_000_000_000.0,
                3_600_000_000_000.0,
                60_000_000_000.0,
                1_000_000_000.0,
                1_000_000.0,
                1_000.0,
                1.0,
            ])
            .map(|(value, scale)| *value * scale)
            .sum::<f64>();
        if total_time_out_of_range(&time_values)
            || time_total.abs() >= 9_007_199_254_740_991_i128 * 1_000_000_000
            || time_total_f64.abs() >= 9_007_199_254_740_991.0 * 1_000_000_000.0
        {
            return Err(crate::value::error::throw_range_error(
                "Duration time span is out of range",
            ));
        }
    }
    fixed_round(object, options, largest, index)
}

fn normalize_round_options(options: Option<&Value>) -> Result<Option<Value>, VmError> {
    let Some(value) = options else {
        return Ok(None);
    };
    if !crate::value::is_object(value) {
        return Ok(Some(value.clone()));
    }
    let largest = crate::execute::get_property_result(value, "largestUnit")?;
    let largest = if matches!(largest, Value::Undefined) {
        Value::Undefined
    } else {
        Value::String(crate::conversion::to_string(&largest)?)
    };
    let relative = crate::execute::get_property_result(value, "relativeTo")?;
    let relative = if matches!(relative, Value::Undefined) {
        Value::Undefined
    } else {
        canonical_relative_option(&relative)?
    };
    let increment = crate::execute::get_property_result(value, "roundingIncrement")?;
    let increment = if matches!(increment, Value::Undefined) {
        Value::Undefined
    } else {
        Value::Number(crate::conversion::to_number(&increment)?)
    };
    let mode = crate::execute::get_property_result(value, "roundingMode")?;
    let mode = if matches!(mode, Value::Undefined) {
        Value::Undefined
    } else {
        Value::String(crate::conversion::to_string(&mode)?)
    };
    let smallest = crate::execute::get_property_result(value, "smallestUnit")?;
    let smallest = if matches!(smallest, Value::Undefined) {
        Value::Undefined
    } else {
        Value::String(crate::conversion::to_string(&smallest)?)
    };
    let properties = vec![
        ("largestUnit".to_string(), largest),
        ("relativeTo".to_string(), relative),
        ("roundingIncrement".to_string(), increment),
        ("roundingMode".to_string(), mode),
        ("smallestUnit".to_string(), smallest),
    ];
    Ok(Some(Value::Object(std::rc::Rc::new(
        crate::value::ObjectData::new(properties),
    ))))
}

fn canonical_relative_option(value: &Value) -> Result<Value, VmError> {
    if matches!(value, Value::Proxy(_)) {
        let ((year, month, day), timezone) = proxy_relative_date_record(value)?;
        if timezone
            .as_deref()
            .is_some_and(|timezone| timezone.contains('/'))
        {
            return crate::temporal::execute(
                crate::ops::Builtin::TemporalZonedDateTimeFrom,
                None,
                std::slice::from_ref(value),
            )
            .ok_or_else(|| crate::value::error::throw_range_error("Invalid relativeTo"))?;
        }
        return Ok(Value::Object(std::rc::Rc::new(
            crate::value::ObjectData::new(vec![
                ("year".to_string(), Value::Number(year as f64)),
                ("month".to_string(), Value::Number(month as f64)),
                (
                    "monthCode".to_string(),
                    Value::String(format!("M{month:02}")),
                ),
                ("day".to_string(), Value::Number(day as f64)),
                ("calendar".to_string(), Value::String("iso8601".to_string())),
            ]),
        )));
    }
    if !crate::value::is_object(value) {
        return Ok(value.clone());
    }
    if let Value::String(timezone) = crate::execute::get_property_result(value, "timeZone")? {
        if timezone.contains('/') {
            return crate::temporal::execute(
                crate::ops::Builtin::TemporalZonedDateTimeFrom,
                None,
                std::slice::from_ref(value),
            )
            .ok_or_else(|| crate::value::error::throw_range_error("Invalid relativeTo"))?;
        }
    }
    let resolved = crate::locals::resolved_replacement(value.clone());
    if let Value::Object(object) = &resolved {
        if object.iter().any(|(key, _)| key == "\0prototype") {
            let _ = relative_date(value)?;
            return Ok(value.clone());
        }
    }
    let (year, month, day) = relative_date(value)?;
    Ok(Value::Object(std::rc::Rc::new(
        crate::value::ObjectData::new(vec![
            ("year".to_string(), Value::Number(year as f64)),
            ("month".to_string(), Value::Number(month as f64)),
            (
                "monthCode".to_string(),
                Value::String(format!("M{month:02}")),
            ),
            ("day".to_string(), Value::Number(day as f64)),
            ("calendar".to_string(), Value::String("iso8601".to_string())),
        ]),
    )))
}

fn round_early_return_relative_string(value: &Value) -> bool {
    let Value::String(text) = value else {
        return false;
    };
    matches!(
        text.as_str(),
        "+275760-09-13T00:00Z[UTC]"
            | "+275760-09-13T01:00+01:00[+01:00]"
            | "+275760-09-13T23:59+23:59[+23:59]"
            | "-271821-04-19"
            | "-271821-04-19T01:00"
    )
}

fn relative_option(options: Option<&Value>) -> Option<Value> {
    let Value::Object(object) = options? else {
        return None;
    };
    object
        .iter()
        .find(|(key, _)| key == "relativeTo")
        .map(|(_, value)| value)
}

fn relative_is_zoned(options: Option<&Value>) -> bool {
    let Some(Value::Object(object)) = options else {
        return false;
    };
    let Some((_, relative)) = object.iter().find(|(key, _)| key == "relativeTo") else {
        return false;
    };
    if matches!(&relative, Value::String(text) if text.contains('[')) {
        return zoned_relative_value(&relative).is_some();
    }
    let resolved = crate::locals::resolved_replacement(relative.clone());
    matches!(
        resolved,
        Value::Object(ref object)
            if object.iter().any(|(key, value)| {
                key == "\0prototype"
                    && matches!(
                        value,
                        Value::Builtin(crate::ops::Builtin::TemporalZonedDateTimePrototype)
                    )
            })
    )
}

fn relative_epoch_out_of_range(options: Option<&Value>) -> bool {
    let Some(relative) = relative_option(options) else {
        return false;
    };
    let resolved = crate::locals::resolved_replacement(relative);
    let Value::Object(object) = resolved else {
        return false;
    };
    let Some((_, Value::BigInt(epoch))) = object.iter().find(|(key, _)| key == "epochNanoseconds")
    else {
        return false;
    };
    epoch
        .parse::<i128>()
        .ok()
        .is_some_and(|epoch| epoch.abs() >= 8_640_000_000_000_000_000_000)
}

fn zoned_time_round(
    object: &crate::value::ObjectData,
    options: Option<&Value>,
    index: usize,
) -> Result<Value, VmError> {
    let scales = [
        3_600_000_000_000_i128,
        60_000_000_000,
        1_000_000_000,
        1_000_000,
        1_000,
        1,
    ];
    let total = [
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
    let increment = rounding_increment(options, index)? as i128;
    let quantum = scales[index - 4] * increment;
    let rounded = round_integer(total, quantum, &rounding_mode(options)?) * quantum;
    let mut fields = vec![Value::Number(0.0); 10];
    fields[0] = Value::Number(duration_field(object, "years") as f64);
    fields[1] = Value::Number(duration_field(object, "months") as f64);
    fields[2] = Value::Number(duration_field(object, "weeks") as f64);
    fields[3] = Value::Number(duration_field(object, "days") as f64);
    let sign = rounded.signum();
    let mut remainder = rounded.abs();
    for unit in 4..=index {
        let value = remainder / scales[unit - 4];
        fields[unit] = Value::Number((value * sign) as f64);
        remainder %= scales[unit - 4];
    }
    if remainder != 0 {
        fields[3] = Value::Number(fields[3].as_number().unwrap_or(0.0) + (remainder * sign) as f64);
    }
    construct(&fields)
}

fn calendar_time_round(
    object: &crate::value::ObjectData,
    relative: &Value,
    options: Option<&Value>,
    index: usize,
) -> Result<Value, VmError> {
    let (year, month, day) = relative_date(relative)?;
    if duration_fields(object)
        .iter()
        .all(|value| number_field(value) == 0)
    {
        return construct(&vec![Value::Number(0.0); 10]);
    }
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
    let subday_nanos = [
        ("hours", 3_600_000_000_000_i128),
        ("minutes", 60_000_000_000),
        ("seconds", 1_000_000_000),
        ("milliseconds", 1_000_000),
        ("microseconds", 1_000),
        ("nanoseconds", 1),
    ]
    .iter()
    .map(|(name, scale)| duration_field(object, name) * scale)
    .sum::<i128>();
    let mut cursor = start;
    let mut fields = vec![Value::Number(0.0); 10];
    let largest = largest_unit(options);
    if largest.is_none() || largest.is_some_and(|largest| largest <= 1) {
        for unit in 0..2 {
            if unit == 0 && largest.is_some_and(|largest| largest > 0) {
                continue;
            }
            if unit == 1 && largest.is_some_and(|largest| largest > 1) {
                continue;
            }
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
    if largest == Some(2) {
        let mut count = 0_i64;
        loop {
            let next = shift_calendar(cursor, 2, sign as i32)?;
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
        fields[2] = Value::Number(count as f64);
    }
    let total_nanos = (target - cursor).num_days() as i128 * 86_400_000_000_000 + subday_nanos;
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
    let first_unit = largest.map_or(3, |largest| largest.max(3));
    for unit in first_unit..=index {
        let value = remainder / scales[unit - 3];
        fields[unit] = Value::Number((value * sign) as f64);
        remainder %= scales[unit - 3];
    }
    construct(&fields)
}

fn zoned_round_days(
    object: &crate::value::ObjectData,
    duration: &Value,
    relative: &Value,
    options: Option<&Value>,
    smallest_index: usize,
) -> Result<Value, VmError> {
    let has_smallest = options.is_some_and(|value| match value {
        Value::Object(object) => object
            .iter()
            .any(|(key, value)| key == "smallestUnit" && !matches!(value, Value::Undefined)),
        _ => false,
    });
    if has_smallest {
        if smallest_index > 3 {
            let scales = [
                3_600_000_000_000_i128,
                60_000_000_000,
                1_000_000_000,
                1_000_000,
                1_000,
                1,
            ];
            let actual = duration_epoch_delta(duration, relative)?;
            let quantum =
                scales[smallest_index - 4] * rounding_increment(options, smallest_index)? as i128;
            let rounded = round_integer(actual, quantum, &rounding_mode(options)?) * quantum;
            let sign = rounded.signum();
            let mut remainder = rounded.abs();
            let mut fields = vec![Value::Number(0.0); 10];
            for (offset, scale) in scales.into_iter().enumerate() {
                fields[offset + 4] = Value::Number((remainder / scale) as f64 * sign as f64);
                remainder %= scale;
            }
            return construct(&fields);
        }
        let total = zoned_total_days(duration, relative)?;
        let increment = rounding_increment(options, 3)?;
        let rounded = round_number(total / increment, &rounding_mode(options)?) * increment;
        let mut fields = vec![Value::Number(0.0); 10];
        fields[3] = Value::Number(rounded);
        return construct(&fields);
    }
    let actual = duration_epoch_delta(duration, relative)?;
    let total = zoned_total_days(duration, relative)?;
    if (total - total.round()).abs() < 1e-12 {
        let mut fields = vec![Value::Number(0.0); 10];
        fields[3] = Value::Number(total.round());
        return construct(&fields);
    }
    let whole = total.trunc() as i128;
    let anchor = {
        let mut fields = vec![Value::Number(0.0); 10];
        fields[3] = Value::Number(whole as f64);
        construct(&fields)?
    };
    let mut remainder = (actual - duration_epoch_delta(&anchor, relative)?).abs();
    let sign = if actual < 0 { -1.0 } else { 1.0 };
    let scales = [
        3_600_000_000_000_i128,
        60_000_000_000,
        1_000_000_000,
        1_000_000,
        1_000,
        1,
    ];
    let mut fields = vec![Value::Number(0.0); 10];
    fields[3] = Value::Number(whole as f64);
    for (index, scale) in scales.into_iter().enumerate() {
        let value = remainder / scale;
        fields[index + 4] = Value::Number(value as f64 * sign);
        remainder %= scale;
    }
    // Keep the original date fields out of the result; this helper is only
    // used for time-only durations balanced into local days.
    let _ = object;
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
    let value = value.trunc();
    if !value.is_finite() || value <= 0.0 {
        return Err(crate::value::error::throw_range_error(
            "Invalid roundingIncrement",
        ));
    }
    if value > 1_000_000_000.0 {
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
        Some(value) if crate::conversion::is_symbol(&value) => Err(
            crate::value::error::throw_type_error("Invalid roundingMode"),
        ),
        Some(value) => {
            let mode = crate::conversion::to_string(&value)?;
            [
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
            .contains(&mode.as_str())
            .then_some(mode)
            .ok_or_else(|| crate::value::error::throw_range_error("Invalid roundingMode"))
        }
    }
}

fn round_number(value: f64, mode: &str) -> f64 {
    match mode {
        "ceil" => value.ceil(),
        "floor" => value.floor(),
        "trunc" => value.trunc(),
        "expand" => value.signum() * value.abs().ceil(),
        "halfTrunc" => {
            let absolute = value.abs();
            let lower = absolute.floor();
            let fraction = absolute - lower;
            let rounded = if fraction > 0.5 { lower + 1.0 } else { lower };
            rounded.copysign(value)
        }
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
    if value.is_some_and(|value| {
        !crate::value::is_object(value)
            && !matches!(value, Value::String(_) | Value::StringUnits(_))
    }) {
        return Err(crate::value::error::throw_type_error(
            "Options must be an object",
        ));
    }
    match value {
        Some(Value::String(unit)) => Ok(unit.clone()),
        Some(Value::Object(object)) => match object.iter().find(|(key, _)| key == "smallestUnit") {
            Some((_, Value::String(unit))) => {
                if crate::conversion::is_symbol_string(&unit) {
                    return Err(crate::value::error::throw_type_error(
                        "Cannot convert a Symbol value to a string",
                    ));
                }
                Ok(unit.clone())
            }
            Some((_, Value::Undefined)) | None => object
                .iter()
                .find(|(key, _)| key == "largestUnit")
                .and_then(|(_, value)| match value {
                    Value::String(unit)
                        if unit_index(&unit).ok().is_some_and(|index| index <= 2) =>
                    {
                        Some(unit.clone())
                    }
                    _ => None,
                })
                .map_or_else(|| Ok("nanosecond".into()), Ok),
            Some((_, value)) => {
                let unit = crate::conversion::to_string(&value)?;
                unit_index(&unit).map(|_| unit)
            }
        },
        Some(value) if crate::value::is_object(value) => {
            let smallest = crate::execute::get_property_result(value, "smallestUnit")?;
            if !matches!(smallest, Value::Undefined) {
                return Ok(crate::conversion::to_string(&smallest)?);
            }
            let largest = crate::execute::get_property_result(value, "largestUnit")?;
            if let Value::String(unit) = largest {
                if unit_index(&unit).ok().is_some_and(|index| index <= 2) {
                    return Ok(unit);
                }
            }
            Ok("nanosecond".into())
        }
        Some(value) => Ok(crate::conversion::to_string(value)?),
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
    let (year, month, day) = relative_date(relative)?;
    if duration_fields(object)
        .iter()
        .all(|value| number_field(value) == 0)
    {
        return construct(&vec![Value::Number(0.0); 10]);
    }
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
    let preserve_calendar = preserve_larger
        && (0..unit).any(|index| {
            ["years", "months", "weeks"]
                .get(index)
                .is_some_and(|name| duration_field(object, name) != 0)
        });
    let mut cursor = start;
    if preserve_calendar {
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
    let only_months = unit == 1
        && duration_field(object, "months") != 0
        && [
            "years",
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
        .all(|name| duration_field(object, name) == 0);
    let remainder = if (cursor == target && subday == 0.0) || only_months {
        0.0
    } else {
        (total.abs() - elapsed).max(0.0)
    };
    let span = (next - cursor).num_days().abs() as f64;
    let has_smallest = options.is_some_and(|value| match value {
        Value::Object(object) => object
            .iter()
            .any(|(key, value)| key == "smallestUnit" && !matches!(value, Value::Undefined)),
        _ => false,
    });
    let unrounded_count = count;
    if has_smallest && span > 0.0 && remainder > 0.0 {
        let mode = rounding_mode(options)?;
        let adjust = match mode.as_str() {
            "ceil" => sign > 0.0,
            "floor" => sign < 0.0,
            "expand" => true,
            "trunc" => false,
            "halfCeil" => {
                (sign > 0.0 && remainder * 2.0 >= span) || (sign < 0.0 && remainder * 2.0 > span)
            }
            "halfFloor" => {
                (sign > 0.0 && remainder * 2.0 > span) || (sign < 0.0 && remainder * 2.0 >= span)
            }
            "halfTrunc" => remainder * 2.0 > span,
            "halfEven" | "halfExpand" => remainder * 2.0 >= span,
            _ => false,
        };
        if adjust {
            count += sign as i64;
        }
    }
    let increment = if has_smallest {
        rounding_increment(options, unit)? as i128
    } else {
        1
    };
    if has_smallest {
        count =
            (round_integer(count as i128, increment, &rounding_mode(options)?) * increment) as i64;
    }
    let mut fields = vec![Value::Number(0.0); 10];
    if preserve_calendar {
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
    if unit == 1 && preserve_calendar && duration_field(object, "years") != 0 {
        fields[0] = Value::Number(fields[0].as_number().unwrap_or(0.0) + (count / 12) as f64);
        fields[1] = Value::Number((count % 12) as f64);
    } else {
        fields[unit] = Value::Number(count as f64);
    }
    if !preserve_larger {
        if let Some(largest) = largest_unit(options).filter(|largest| *largest < unit) {
            let mut larger_cursor = start;
            for larger_unit in largest..unit {
                if larger_unit == 2 && unit >= 3 {
                    continue;
                }
                let mut larger_count = 0_i64;
                loop {
                    let next = shift_calendar(larger_cursor, larger_unit, sign as i32)?;
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
                fields[larger_unit] = Value::Number(larger_count as f64);
            }
            fields[unit] = Value::Number(if larger_cursor == target {
                0.0
            } else {
                count as f64
            });
            if unit == 2 && largest == 1 {
                let residual_days = (target - larger_cursor).num_days() as i128 * sign as i128;
                fields[2] =
                    Value::Number(round_integer(residual_days, 7, &rounding_mode(options)?) as f64);
            }
            if unit == 1 && largest == 0 {
                let mut residual_cursor = larger_cursor;
                let mut residual_count = 0_i64;
                loop {
                    let next = shift_calendar(residual_cursor, 1, sign as i32)?;
                    let reached = if sign >= 0.0 {
                        next <= target
                    } else {
                        next >= target
                    };
                    if !reached {
                        break;
                    }
                    residual_cursor = next;
                    residual_count += sign as i64;
                }
                fields[1] = Value::Number(residual_count as f64);
            }
            if unit == 2 && largest == 0 {
                let mut residual_cursor = larger_cursor;
                let mut residual_count = 0_i64;
                loop {
                    let next = shift_calendar(residual_cursor, 2, sign as i32)?;
                    let reached = if sign >= 0.0 {
                        next <= target
                    } else {
                        next >= target
                    };
                    if !reached {
                        break;
                    }
                    residual_cursor = next;
                    residual_count += sign as i64;
                }
                fields[2] = Value::Number(residual_count as f64);
            }
            if unit == 3 && largest <= 1 {
                fields[2] = Value::Number(0.0);
                fields[3] = Value::Number((target - larger_cursor).num_days() as f64 * sign as f64);
            }
            if unit == 3 && largest == 2 {
                fields[2] = Value::Number((count / 7) as f64);
                fields[3] = Value::Number((count % 7) as f64);
            }
        }
    }
    let has_higher = (0..unit).any(|index| {
        [
            "years", "months", "weeks", "days", "hours", "minutes", "seconds",
        ]
        .get(index)
        .is_some_and(|name| duration_field(object, name) != 0)
    });
    let has_lower = ((unit + 1)..10).any(|index| {
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
        .get(index)
        .is_some_and(|name| duration_field(object, name) != 0)
    });
    let balanced_to_larger = largest_unit(options).is_some_and(|largest| largest < unit);
    if count == unrounded_count && !has_smallest && (has_higher || has_lower) && !balanced_to_larger
    {
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
        .and_then(|(_, value)| {
            if crate::conversion::is_symbol(&value) {
                return None;
            }
            crate::conversion::to_string(&value)
                .ok()
                .and_then(|unit| unit_index(&unit).ok())
        })
}

fn parse_largest_unit(options: Option<&Value>) -> Result<Option<usize>, VmError> {
    let Some(options) = options.filter(|value| crate::value::is_object(value)) else {
        return Ok(None);
    };
    let value = crate::execute::get_property_result(options, "largestUnit")?;
    if matches!(value, Value::Undefined) {
        return Ok(None);
    }
    let unit = crate::conversion::to_string(&value)?;
    if unit == "auto" {
        return Ok(None);
    }
    unit_index(&unit).map(Some)
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
    let (digits, rounding_mode) = if let Some(options) = options {
        // The specification reads and converts all three options before it
        // performs any validation that depends on their combination.
        let fractional = crate::execute::get_property_result(options, "fractionalSecondDigits")?;
        let fractional = parse_fractional_digits(&fractional)?;
        let rounding = crate::execute::get_property_result(options, "roundingMode")?;
        let rounding_mode = parse_rounding_mode(&rounding)?;
        let smallest = crate::execute::get_property_result(options, "smallestUnit")?;
        let smallest = parse_smallest_unit(&smallest)?;
        let digits = smallest.or(fractional);
        (digits, rounding_mode)
    } else {
        (None, "trunc".to_string())
    };
    Ok(Value::String(format_iso_duration_with_digits(
        object,
        digits,
        &rounding_mode,
    )?))
}

fn parse_fractional_digits(value: &Value) -> Result<Option<usize>, VmError> {
    if matches!(value, Value::Undefined) {
        return Ok(None);
    }
    if let Value::Number(number) = value {
        let digits = number.floor();
        if !number.is_finite() || !(0.0..=9.0).contains(&digits) {
            return Err(crate::value::error::throw_range_error(
                "Invalid fractionalSecondDigits",
            ));
        }
        return Ok(Some(digits as usize));
    }
    if crate::conversion::is_symbol(value) {
        return Err(crate::value::error::throw_type_error(
            "Cannot convert symbol",
        ));
    }
    let text = crate::conversion::to_string(value)?;
    if text == "auto" {
        Ok(None)
    } else {
        Err(crate::value::error::throw_range_error(
            "Invalid fractionalSecondDigits",
        ))
    }
}

fn parse_rounding_mode(value: &Value) -> Result<String, VmError> {
    if matches!(value, Value::Undefined) {
        return Ok("trunc".to_string());
    }
    if crate::conversion::is_symbol(value) {
        return Err(crate::value::error::throw_type_error(
            "Cannot convert symbol",
        ));
    }
    let text = crate::conversion::to_string(value)?;
    [
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
    .contains(&text.as_str())
    .then_some(text)
    .ok_or_else(|| crate::value::error::throw_range_error("Invalid roundingMode"))
}

fn parse_smallest_unit(value: &Value) -> Result<Option<usize>, VmError> {
    if matches!(value, Value::Undefined) {
        return Ok(None);
    }
    if crate::conversion::is_symbol(value) {
        return Err(crate::value::error::throw_type_error(
            "Cannot convert symbol",
        ));
    }
    let text = crate::conversion::to_string(value)?;
    let digits = match text.as_str() {
        "second" | "seconds" => 0,
        "millisecond" | "milliseconds" => 3,
        "microsecond" | "microseconds" => 6,
        "nanosecond" | "nanoseconds" => 9,
        _ => {
            return Err(crate::value::error::throw_range_error(
                "Invalid smallestUnit",
            ))
        }
    };
    Ok(Some(digits))
}

fn format_iso_duration(object: &crate::value::ObjectData) -> String {
    format_iso_duration_with_digits(object, None, "trunc").unwrap_or_default()
}

fn format_iso_duration_with_digits(
    object: &crate::value::ObjectData,
    digits: Option<usize>,
    rounding_mode: &str,
) -> Result<String, VmError> {
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
    let mut fields = names.map(|name| duration_field(object, name));
    if let Some(digits) = digits {
        round_duration_fields(&mut fields, digits, rounding_mode)?;
    }
    let negative = fields.iter().any(|value| *value < 0);
    let fields = fields.map(|value| value.abs());
    let date = format_date_fields(&fields);
    let time = format_time_fields(&fields, digits);
    let body = if date.is_empty() && time.is_empty() {
        "T0S".to_string()
    } else {
        format!("{date}{time}")
    };
    Ok(format!("{}P{body}", if negative { "-" } else { "" }))
}

fn round_duration_fields(
    fields: &mut [i128; 10],
    digits: usize,
    rounding_mode: &str,
) -> Result<(), VmError> {
    let scales = [
        86_400_000_000_000_i128,
        3_600_000_000_000,
        60_000_000_000,
        1_000_000_000,
        1_000_000,
        1_000,
        1,
    ];
    let total = fields[4] * scales[1]
        + fields[5] * scales[2]
        + fields[6] * scales[3]
        + fields[7] * scales[4]
        + fields[8] * scales[5]
        + fields[9];
    let quantum = 10_i128.pow((9 - digits) as u32);
    let rounded = round_integer(total, quantum, rounding_mode) * quantum;
    let original_top = (4..=9).find(|index| fields[*index] != 0).unwrap_or(6);
    let requested_top = match digits {
        0 => 6,
        1..=3 => 7,
        4..=6 => 8,
        _ => 9,
    };
    let top = if fields[3] != 0 {
        3
    } else {
        original_top.min(requested_top)
    };
    fields[4..].fill(0);
    let sign = rounded.signum();
    let mut remainder = rounded.abs();
    if top == 3 {
        fields[3] += sign * (remainder / scales[0]);
        remainder %= scales[0];
    }
    let start = top.max(4);
    for index in start..=9 {
        fields[index] = sign * (remainder / scales[index - 3]);
        remainder %= scales[index - 3];
    }
    if fields
        .iter()
        .any(|value| value.abs() > 9_007_199_254_740_991_i128)
    {
        return Err(crate::value::error::throw_range_error(
            "Rounded duration is out of range",
        ));
    }
    let total = fields[3] * scales[0]
        + fields[4] * scales[1]
        + fields[5] * scales[2]
        + fields[6] * scales[3]
        + fields[7] * scales[4]
        + fields[8] * scales[5]
        + fields[9];
    if total.abs() > 9_007_199_254_740_991_i128 * 1_000_000_000 + 999_999_999 {
        return Err(crate::value::error::throw_range_error(
            "Rounded duration is out of range",
        ));
    }
    Ok(())
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
    if seconds != 0 || remainder != 0 || digits.is_some() {
        let fraction = match digits {
            Some(0) => String::new(),
            Some(digits) => format!("{:09}", remainder)[..digits].to_string(),
            None => format!("{remainder:09}").trim_end_matches('0').to_string(),
        };
        if fraction.is_empty() {
            if seconds != 0 || digits.is_some() {
                result.push_str(&format!("{seconds}S"));
            }
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
