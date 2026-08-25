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
        crate::ops::Builtin::TemporalDurationAbs => Some(abs(receiver)),
        crate::ops::Builtin::TemporalDurationToLocaleString => Some(
            crate::intl::duration::format_temporal_duration(receiver, arguments),
        ),
        crate::ops::Builtin::TemporalDurationToJSON => Some(to_json(receiver)),
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
