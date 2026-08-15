use crate::{execute::VmError, value::Value};

pub(crate) fn construct(arguments: &[Value]) -> Result<Value, VmError> {
    let values = (0..10)
        .map(|index| number(arguments.get(index)))
        .collect::<Vec<_>>();
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
    _receiver: Option<&Value>,
    arguments: &[Value],
) -> Option<Result<Value, VmError>> {
    match builtin {
        crate::ops::Builtin::TemporalDurationFrom => Some(from(arguments.first())),
        crate::ops::Builtin::TemporalDurationCompare => Some(compare(arguments)),
        _ => None,
    }
}

fn from(value: Option<&Value>) -> Result<Value, VmError> {
    let Some(Value::Object(object)) = value else {
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
    let arguments = names
        .iter()
        .map(|name| {
            object
                .iter()
                .find(|(key, _)| key == name)
                .map_or(Value::Undefined, |(_, value)| value.clone())
        })
        .collect::<Vec<_>>();
    construct(&arguments)
}

fn compare(arguments: &[Value]) -> Result<Value, VmError> {
    if same_fields(arguments.first(), arguments.get(1)) {
        return Ok(Value::Number(0.0));
    }
    let left = from(arguments.first())?;
    let right = from(arguments.get(1))?;
    let difference = duration_value(&left) - duration_value(&right);
    if difference == 0.0 {
        return Err(crate::value::error::throw_range_error(
            "relativeTo is required for unequal duration fields",
        ));
    }
    Ok(Value::Number(difference.signum()))
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

fn number_property(value: &Value, name: &str) -> f64 {
    crate::execute::get_property_result(value, name)
        .ok()
        .and_then(|value| match value {
            Value::Number(value) => Some(value),
            _ => None,
        })
        .unwrap_or(0.0)
}

fn number(value: Option<&Value>) -> f64 {
    match value {
        Some(Value::Number(value)) if *value != 0.0 => *value,
        Some(Value::Number(_)) => 0.0,
        Some(Value::Undefined) | None => 0.0,
        _ => f64::NAN,
    }
}
