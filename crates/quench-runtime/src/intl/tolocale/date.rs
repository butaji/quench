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
    let formatter =
        crate::intl::datetime::construct_with_defaults(arguments, Some(default_components(kind)))?;
    crate::intl::datetime::prototype_method(
        Builtin::IntlDateTimeFormatFormat,
        &[receiver.cloned().unwrap_or(Value::Undefined)],
        Some(&formatter),
    )
}

fn is_date_receiver(receiver: Option<&Value>) -> bool {
    date_time_value(receiver).is_some()
}

fn is_invalid_date(receiver: Option<&Value>) -> bool {
    date_time_value(receiver).is_some_and(|value| value.is_nan())
}

fn date_time_value(receiver: Option<&Value>) -> Option<f64> {
    let Value::Object(properties) = receiver? else {
        return None;
    };
    let (_, Value::BindingCell(cell)) = properties.iter().find(|(name, _)| name == "timeValue")?
    else {
        return None;
    };
    match &*cell.borrow() {
        Value::Number(value) => Some(*value),
        _ => None,
    }
}

fn default_components(kind: DateLocaleKind) -> &'static [&'static str] {
    match kind {
        DateLocaleKind::String => &["year", "month", "day", "hour", "minute", "second"],
        DateLocaleKind::Date => &["year", "month", "day"],
        DateLocaleKind::Time => &["hour", "minute", "second"],
    }
}
