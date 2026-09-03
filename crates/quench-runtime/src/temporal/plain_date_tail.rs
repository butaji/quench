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
    date_object_with_calendar(year, month, day, "iso8601")
}

pub(super) fn date_object_with_calendar(year: f64, month: f64, day: f64, calendar: &str) -> Value {
    let month_code = format!("M{month:02.0}");
    Value::Object(std::rc::Rc::new(crate::value::ObjectData::new(vec![
        ("year".into(), Value::Number(year)),
        ("\0temporal-slot:\0year".into(), Value::Number(year)),
        ("month".into(), Value::Number(month)),
        ("\0temporal-slot:\0month".into(), Value::Number(month)),
        ("monthCode".into(), Value::String(month_code.clone())),
        (
            "\0temporal-slot:\0monthCode".into(),
            Value::String(month_code),
        ),
        ("day".into(), Value::Number(day)),
        ("\0temporal-slot:\0day".into(), Value::Number(day)),
        ("calendarId".into(), Value::String(calendar.to_string())),
        ("\0temporal-plain-date".into(), Value::Boolean(true)),
        (
            "\0prototype".into(),
            Value::Builtin(crate::ops::Builtin::TemporalPlainDatePrototype),
        ),
    ])))
}
