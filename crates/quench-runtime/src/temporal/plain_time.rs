use crate::{execute::VmError, value::Value};

pub(crate) fn construct(arguments: &[Value]) -> Result<Value, VmError> {
    let values = (0..6)
        .map(|index| number(arguments.get(index)))
        .map(|value| value.map(f64::trunc))
        .collect::<Result<Vec<_>, _>>()?;
    if values.iter().any(|value| !value.is_finite()) {
        return Err(crate::value::error::throw_range_error("Invalid time"));
    }
    if !(0.0..=23.0).contains(&values[0])
        || !(0.0..=59.0).contains(&values[1])
        || !(0.0..=59.0).contains(&values[2])
        || !(0.0..=999.0).contains(&values[3])
        || !(0.0..=999.0).contains(&values[4])
        || !(0.0..=999.0).contains(&values[5])
    {
        return Err(crate::value::error::throw_range_error("Invalid time"));
    }
    Ok(Value::Object(std::rc::Rc::new(
        crate::value::ObjectData::new(vec![
            ("hour".into(), Value::Number(values[0])),
            ("minute".into(), Value::Number(values[1])),
            ("second".into(), Value::Number(values[2])),
            ("millisecond".into(), Value::Number(values[3])),
            ("microsecond".into(), Value::Number(values[4])),
            ("nanosecond".into(), Value::Number(values[5])),
            (
                "\0prototype".into(),
                Value::Builtin(crate::ops::Builtin::TemporalPlainTimePrototype),
            ),
        ]),
    )))
}

pub(crate) fn execute(
    builtin: crate::ops::Builtin,
    _receiver: Option<&Value>,
    arguments: &[Value],
) -> Option<Result<Value, VmError>> {
    match builtin {
        crate::ops::Builtin::TemporalPlainTimeFrom => Some(from(arguments.first())),
        crate::ops::Builtin::TemporalPlainTimeCompare => Some(compare(arguments)),
        _ => None,
    }
}

fn from(value: Option<&Value>) -> Result<Value, VmError> {
    let Some(value) = value else {
        return Err(crate::value::error::throw_type_error("Invalid time"));
    };
    if let Value::String(text) = value {
        return parse_string(text);
    }
    if !crate::value::is_object(value) {
        return Err(crate::value::error::throw_type_error("Invalid time"));
    }
    let values = [
        "hour",
        "minute",
        "second",
        "millisecond",
        "microsecond",
        "nanosecond",
    ]
    .iter()
    .map(|name| crate::execute::get_property_result(value, name))
    .collect::<Result<Vec<_>, _>>()?;
    if matches!(values.first(), Some(Value::Undefined)) {
        return Err(crate::value::error::throw_type_error("Missing hour"));
    }
    construct(&values)
}

fn parse_string(text: &str) -> Result<Value, VmError> {
    let time = text.split('[').next().unwrap_or(text);
    let parts = time.split(':').collect::<Vec<_>>();
    if parts.len() < 2 || parts.len() > 3 {
        return Err(crate::value::error::throw_range_error("Invalid time"));
    }
    let hour = parts[0]
        .parse::<f64>()
        .map_err(|_| crate::value::error::throw_range_error("Invalid time"))?;
    let minute = parts[1]
        .parse::<f64>()
        .map_err(|_| crate::value::error::throw_range_error("Invalid time"))?;
    let (second, fraction) = parts.get(2).map_or((0.0, 0.0), |part| {
        part.split_once('.').map_or_else(
            || (part.parse().unwrap_or(f64::NAN), 0.0),
            |(whole, fraction)| {
                let second = whole.parse().unwrap_or(f64::NAN);
                let digits = fraction.chars().take(9).collect::<String>();
                let nanos = format!("{digits:0<9}").parse::<f64>().unwrap_or(f64::NAN);
                (second, nanos)
            },
        )
    });
    construct(&[
        Value::Number(hour),
        Value::Number(minute),
        Value::Number(second),
        Value::Number((fraction / 1_000_000.0).trunc()),
        Value::Number((fraction / 1_000.0).trunc() % 1_000.0),
        Value::Number(fraction % 1_000.0),
    ])
}

fn compare(arguments: &[Value]) -> Result<Value, VmError> {
    let left = from(arguments.first())?;
    let right = from(arguments.get(1))?;
    let left = time_fields(&left)?;
    let right = time_fields(&right)?;
    Ok(Value::Number((left.cmp(&right) as i8) as f64))
}

fn time_fields(value: &Value) -> Result<i64, VmError> {
    let names = [
        "hour",
        "minute",
        "second",
        "millisecond",
        "microsecond",
        "nanosecond",
    ];
    let values = names
        .iter()
        .map(|name| crate::execute::get_property_result(value, name))
        .collect::<Result<Vec<_>, _>>()?;
    let values = values
        .iter()
        .map(|value| crate::conversion::to_number(value).map(|value| value as i64))
        .collect::<Result<Vec<_>, _>>()?;
    Ok(values[0] * 3_600_000_000_000
        + values[1] * 60_000_000_000
        + values[2] * 1_000_000_000
        + values[3] * 1_000_000
        + values[4] * 1_000
        + values[5])
}

fn number(value: Option<&Value>) -> Result<f64, VmError> {
    match value {
        None | Some(Value::Undefined) => Ok(0.0),
        Some(value) => crate::conversion::to_number(value),
    }
}
