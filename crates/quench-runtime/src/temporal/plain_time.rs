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
    _builtin: crate::ops::Builtin,
    _receiver: Option<&Value>,
    _arguments: &[Value],
) -> Option<Result<Value, VmError>> {
    None
}

fn number(value: Option<&Value>) -> Result<f64, VmError> {
    match value {
        None | Some(Value::Undefined) => Ok(0.0),
        Some(value) => crate::conversion::to_number(value),
    }
}
