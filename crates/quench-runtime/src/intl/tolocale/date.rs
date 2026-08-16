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
    matches!(receiver, Some(Value::Object(properties)) if properties.iter().any(|(name, _)| name == "timeValue"))
}

fn is_invalid_date(receiver: Option<&Value>) -> bool {
    let Some(Value::Object(properties)) = receiver else {
        return false;
    };
    properties.iter().any(|(name, _)| name == "timeValue")
        && crate::date::extract_time(receiver).is_nan()
}

fn default_components(kind: DateLocaleKind) -> &'static [&'static str] {
    match kind {
        DateLocaleKind::String => &["year", "month", "day", "hour", "minute", "second"],
        DateLocaleKind::Date => &["year", "month", "day"],
        DateLocaleKind::Time => &["hour", "minute", "second"],
    }
}
