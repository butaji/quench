use crate::value::Value;

use crate::execute::VmError;

pub(super) fn number(value: Option<&Value>) -> Result<f64, VmError> {
    let value = crate::conversion::to_number(value.unwrap_or(&Value::Undefined))?;
    if !value.is_finite() {
        return Err(crate::value::error::throw_range_error("Invalid PlainDate"));
    }
    Ok(value.trunc())
}

pub(super) fn date_object(year: f64, month: f64, day: f64) -> Value {
    Value::Object(std::rc::Rc::new(crate::value::ObjectData::new(vec![
        ("year".into(), Value::Number(year)),
        ("month".into(), Value::Number(month)),
        ("monthCode".into(), Value::String(format!("M{month:02.0}"))),
        ("day".into(), Value::Number(day)),
        ("calendarId".into(), Value::String("iso8601".into())),
        (
            "\0prototype".into(),
            Value::Builtin(crate::ops::Builtin::TemporalPlainDatePrototype),
        ),
    ])))
}
