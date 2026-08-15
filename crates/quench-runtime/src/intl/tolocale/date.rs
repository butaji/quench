use crate::{execute::VmError, ops::Builtin, value::Value};

use super::DateLocaleKind;

pub(super) fn to_locale_string(
    kind: DateLocaleKind,
    receiver: Option<&Value>,
    arguments: &[Value],
) -> Result<Value, VmError> {
    if !is_date_receiver(receiver) {
        return Err(crate::value::error::throw_type_error(
            "Date.prototype.toLocaleString called on incompatible receiver",
        ));
    }
    if is_invalid_date(receiver) {
        return Ok(Value::String("Invalid Date".into()));
    }
    let mut formatter_arguments = arguments.to_vec();
    if formatter_arguments.len() < 2 {
        formatter_arguments.push(Value::Object(std::rc::Rc::new(
            crate::value::ObjectData::new(default_options(kind)),
        )));
    }
    let formatter = crate::intl::datetime::construct(&formatter_arguments)?;
    crate::intl::datetime::prototype_method(
        Builtin::IntlDateTimeFormatFormat,
        &[receiver.cloned().unwrap_or(Value::Undefined)],
        Some(&formatter),
    )
}

fn is_date_receiver(receiver: Option<&Value>) -> bool {
    matches!(receiver, Some(Value::Object(properties)) if properties.iter().any(|(name, _)| name == "timeValue"))
}

fn is_invalid_date(receiver: Option<&Value>) -> bool {
    let Some(Value::Object(properties)) = receiver else {
        return false;
    };
    properties.iter().any(|(name, _)| name == "timeValue")
        && crate::date::extract_time(receiver).is_nan()
}

fn default_options(kind: DateLocaleKind) -> Vec<(String, Value)> {
    let names = match kind {
        DateLocaleKind::String => ["year", "month", "day", "hour", "minute", "second"].as_slice(),
        DateLocaleKind::Date => ["year", "month", "day"].as_slice(),
        DateLocaleKind::Time => ["hour", "minute", "second"].as_slice(),
    };
    names
        .iter()
        .map(|name| (name.to_string(), Value::String("numeric".into())))
        .collect()
}
