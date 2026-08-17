use crate::{execute::VmError, value::Value};

use super::{duration_arithmetic, duration_format, duration_parse};

pub(super) use duration_parse::from;

pub(crate) use super::duration_construct::construct;

pub(crate) fn execute(
    builtin: crate::ops::Builtin,
    receiver: Option<&Value>,
    arguments: &[Value],
) -> Option<Result<Value, VmError>> {
    match builtin {
        crate::ops::Builtin::TemporalDurationFrom => Some(duration_parse::from(arguments.first())),
        crate::ops::Builtin::TemporalDurationCompare => Some(compare(arguments)),
        crate::ops::Builtin::TemporalDurationAdd => Some(duration_arithmetic::combine(
            receiver,
            arguments.first(),
            1.0,
        )),
        crate::ops::Builtin::TemporalDurationSubtract => Some(duration_arithmetic::combine(
            receiver,
            arguments.first(),
            -1.0,
        )),
        crate::ops::Builtin::TemporalDurationAbs => Some(abs(receiver)),
        crate::ops::Builtin::TemporalDurationNegated => Some(negated(receiver)),
        crate::ops::Builtin::TemporalDurationRound => Some(round(receiver, arguments)),
        crate::ops::Builtin::TemporalDurationToLocaleString => Some(
            crate::intl::duration::format_temporal_duration(receiver, arguments),
        ),
        crate::ops::Builtin::TemporalDurationToJSON => Some(duration_format::to_json(receiver)),
        crate::ops::Builtin::TemporalDurationToString => Some(duration_format::to_string(receiver)),
        _ => access_execute(builtin, receiver),
    }
}

fn access_execute(
    builtin: crate::ops::Builtin,
    receiver: Option<&Value>,
) -> Option<Result<Value, VmError>> {
    let field = match builtin {
        crate::ops::Builtin::TemporalDurationYearsGetter => "years",
        crate::ops::Builtin::TemporalDurationMonthsGetter => "months",
        crate::ops::Builtin::TemporalDurationWeeksGetter => "weeks",
        crate::ops::Builtin::TemporalDurationDaysGetter => "days",
        crate::ops::Builtin::TemporalDurationHoursGetter => "hours",
        crate::ops::Builtin::TemporalDurationMinutesGetter => "minutes",
        crate::ops::Builtin::TemporalDurationSecondsGetter => "seconds",
        crate::ops::Builtin::TemporalDurationMillisecondsGetter => "milliseconds",
        crate::ops::Builtin::TemporalDurationMicrosecondsGetter => "microseconds",
        crate::ops::Builtin::TemporalDurationNanosecondsGetter => "nanoseconds",
        crate::ops::Builtin::TemporalDurationSignGetter => "sign",
        _ => return special_access_execute(builtin, receiver),
    };
    Some(field_getter(receiver, field))
}

fn special_access_execute(
    builtin: crate::ops::Builtin,
    receiver: Option<&Value>,
) -> Option<Result<Value, VmError>> {
    match builtin {
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

fn negated(receiver: Option<&Value>) -> Result<Value, VmError> {
    let object = duration_receiver(receiver)?;
    let arguments = duration_fields(object, false)
        .into_iter()
        .map(|value| match value {
            Value::Number(value) => Value::Number(-value),
            value => value,
        })
        .collect::<Vec<_>>();
    construct(&arguments)
}

fn round(receiver: Option<&Value>, arguments: &[Value]) -> Result<Value, VmError> {
    let object = duration_receiver(receiver)?;
    let unit = round_unit(arguments.first())?;
    validate_rounding_increment(arguments.first())?;
    let mut fields = duration_fields(object, false);
    let first = unit_index(unit)?;
    for field in fields.iter_mut().skip(first + 1) {
        *field = Value::Number(0.0);
    }
    construct(&fields)
}

fn validate_rounding_increment(value: Option<&Value>) -> Result<(), VmError> {
    let Some(Value::Object(object)) = value else {
        return Ok(());
    };
    let Some((_, Value::Number(increment))) =
        object.iter().find(|(key, _)| key == "roundingIncrement")
    else {
        return Ok(());
    };
    if increment.is_nan() {
        return Err(crate::value::error::throw_range_error(
            "Invalid roundingIncrement",
        ));
    }
    Ok(())
}

fn round_unit(value: Option<&Value>) -> Result<&str, VmError> {
    match value {
        Some(Value::String(unit)) => Ok(unit.as_str()),
        Some(Value::Object(object)) => object
            .iter()
            .find(|(key, _)| key == "smallestUnit")
            .and_then(|(_, value)| match value {
                Value::String(unit) => Some(unit.as_str()),
                _ => None,
            })
            .ok_or_else(|| crate::value::error::throw_range_error("Invalid smallestUnit")),
        _ => Err(crate::value::error::throw_range_error(
            "Invalid smallestUnit",
        )),
    }
}

fn unit_index(unit: &str) -> Result<usize, VmError> {
    [
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

pub(super) fn duration_field(object: &crate::value::ObjectData, name: &str) -> i128 {
    object
        .iter()
        .find(|(key, _)| key == name)
        .map_or(0, |(_, value)| number_field(value))
}

fn number_field(value: &Value) -> i128 {
    match value {
        Value::Number(value) => *value as i128,
        _ => 0,
    }
}

pub(super) fn duration_receiver(
    receiver: Option<&Value>,
) -> Result<&crate::value::ObjectData, VmError> {
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
    duration_fields(object, true)
}

fn duration_fields(object: &crate::value::ObjectData, absolute: bool) -> Vec<Value> {
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
                    Value::Number(value) => {
                        Value::Number(if absolute { value.abs() } else { *value })
                    }
                    _ => Value::Number(0.0),
                })
        })
        .collect()
}

pub(crate) use super::duration_parse::parse_string;

fn compare(arguments: &[Value]) -> Result<Value, VmError> {
    validate_compare_options(arguments.get(2))?;
    if same_fields(arguments.first(), arguments.get(1)) {
        return Ok(Value::Number(0.0));
    }
    let left = from(arguments.first())?;
    let right = from(arguments.get(1))?;
    let relative_to_missing = arguments
        .get(2)
        .is_none_or(|value| matches!(value, Value::Undefined));
    if (date_units(&left) || date_units(&right)) && relative_to_missing {
        return Err(crate::value::error::throw_range_error(
            "relativeTo is required for date units",
        ));
    }
    let difference = if date_units(&left) || date_units(&right) {
        duration_value(&left) - duration_value(&right)
    } else {
        exact_time_difference(&left, &right)
    };
    if difference == 0.0 {
        return Ok(Value::Number(0.0));
    }
    Ok(Value::Number(difference.signum()))
}

fn validate_compare_options(options: Option<&Value>) -> Result<(), VmError> {
    if let Some(options) = options {
        if !matches!(options, Value::Undefined) && !crate::value::is_object(options) {
            return Err(crate::value::error::throw_type_error(
                "Duration.compare options must be an object",
            ));
        }
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

fn duration_value(value: &Value) -> f64 {
    [
        ("years", 31_536_000.0),
        ("months", 2_592_000.0),
        ("weeks", 604_800.0),
        ("days", 86_400.0),
        ("hours", 3_600.0),
        ("minutes", 60.0),
        ("seconds", 1.0),
        ("milliseconds", 1e-3),
        ("microseconds", 1e-6),
        ("nanoseconds", 1e-9),
    ]
    .iter()
    .map(|(name, scale)| number_property(value, name) * scale)
    .sum()
}

fn exact_time_difference(left: &Value, right: &Value) -> f64 {
    let left = time_nanoseconds(left);
    let right = time_nanoseconds(right);
    match left.cmp(&right) {
        std::cmp::Ordering::Less => -1.0,
        std::cmp::Ordering::Equal => 0.0,
        std::cmp::Ordering::Greater => 1.0,
    }
}

fn time_nanoseconds(value: &Value) -> i128 {
    [
        ("days", 86_400_000_000_000_i128),
        ("hours", 3_600_000_000_000),
        ("minutes", 60_000_000_000),
        ("seconds", 1_000_000_000),
        ("milliseconds", 1_000_000),
        ("microseconds", 1_000),
        ("nanoseconds", 1),
    ]
    .iter()
    .map(|(name, scale)| i128::from(number_property(value, name) as i64) * scale)
    .sum()
}

pub(super) fn validate_range(values: &[f64]) -> Result<(), VmError> {
    if values.iter().any(|value| value.fract() != 0.0)
        || mixed_signs(values)
        || date_fields_out_of_range(&values[..3])
        || time_fields_out_of_range(&values[3..])
    {
        return Err(crate::value::error::throw_range_error(
            "Duration fields are out of range",
        ));
    }
    Ok(())
}

fn mixed_signs(values: &[f64]) -> bool {
    let Some(sign) = values
        .iter()
        .find(|value| **value != 0.0)
        .map(|value| value.signum())
    else {
        return false;
    };
    values
        .iter()
        .any(|value| *value != 0.0 && value.signum() != sign)
}

fn date_fields_out_of_range(values: &[f64]) -> bool {
    values.iter().any(|value| value.abs() > 4_294_967_295.0)
}

fn time_fields_out_of_range(values: &[f64]) -> bool {
    let limits = [
        104_249_991_375.0,
        2_501_999_792_984.0,
        150_119_987_579_017.0,
        9_007_199_254_740_991.0,
        9_007_199_254_740_991.0,
        9_007_199_254_740_991.0,
        9_007_199_254_740_991.0,
    ];
    values
        .iter()
        .zip(limits)
        .any(|(value, limit)| value.abs() > limit)
        || total_time_out_of_range(values)
}

fn total_time_out_of_range(values: &[f64]) -> bool {
    let scales = [
        86_400_000_000_000_i128,
        3_600_000_000_000,
        60_000_000_000,
        1_000_000_000,
        1_000_000,
        1_000,
        1,
    ];
    let total = values
        .iter()
        .zip(scales)
        .try_fold(0_i128, |total, (value, scale)| {
            let value = i128::from(*value as i64);
            total.checked_add(value.checked_mul(scale)?)
        });
    let limit = 9_007_199_254_740_991_i128 * 1_000_000_000 + 999_999_999;
    total.is_none_or(|total| total.abs() > limit)
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
