use crate::{execute::VmError, value::Value};

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
        crate::ops::Builtin::TemporalDurationAbs => Some(abs(receiver)),
        _ => None,
    }
}

fn abs(receiver: Option<&Value>) -> Result<Value, VmError> {
    let object = duration_receiver(receiver)?;
    let arguments = absolute_fields(object);
    construct(&arguments)
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
    if (date_units(&left) || date_units(&right)) && arguments.get(2).is_none() {
        return Err(crate::value::error::throw_range_error(
            "relativeTo is required for date units",
        ));
    }
    let difference = duration_value(&left) - duration_value(&right);
    if difference == 0.0 {
        return Err(crate::value::error::throw_range_error(
            "relativeTo is required for unequal duration fields",
        ));
    }
    Ok(Value::Number(difference.signum()))
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

fn validate_range(values: &[f64]) -> Result<(), VmError> {
    if date_fields_out_of_range(&values[..3]) || time_fields_out_of_range(&values[3..]) {
        return Err(crate::value::error::throw_range_error(
            "Duration fields are out of range",
        ));
    }
    Ok(())
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

fn number(value: Option<&Value>) -> Result<f64, VmError> {
    let value = match value {
        Some(Value::Number(value)) => Ok(*value),
        Some(Value::Undefined) | None => Ok(0.0),
        Some(value) => crate::conversion::to_number(value),
    }?;
    if !value.is_finite() || value.fract() != 0.0 {
        return Err(crate::value::error::throw_range_error(
            "Duration fields must be integral",
        ));
    }
    Ok(if value == 0.0 { 0.0 } else { value })
}
