fn receiver_slots(receiver: Option<&Value>) -> Result<Vec<(String, Value)>, VmError> {
    super::intl_slots(receiver)
}

pub(crate) fn dispatch(
    builtin: crate::ops::Builtin,
    arguments: &[Value],
    receiver: Option<&Value>,
) -> Option<Result<Value, VmError>> {
    match builtin {
        crate::ops::Builtin::IntlRelativeTimeFormat => Some(construct_if_new(arguments, receiver)),
        crate::ops::Builtin::IntlRelativeTimeFormatSupportedLocalesOf => {
            Some(super::supported_locales_of(arguments))
        }
        crate::ops::Builtin::IntlRelativeTimeFormatFormat
        | crate::ops::Builtin::IntlRelativeTimeFormatFormatToParts
        | crate::ops::Builtin::IntlRelativeTimeFormatResolvedOptions => {
            Some(prototype_method(builtin, arguments, receiver))
        }
        _ => None,
    }
}

fn construct_if_new(arguments: &[Value], receiver: Option<&Value>) -> Result<Value, VmError> {
    if receiver.is_some() {
        return Err(runtime_error(
            "TypeError: Intl.RelativeTimeFormat requires 'new'",
        ));
    }
    construct(arguments)
}

fn relative_parts(
    value: f64,
    unit: &str,
    style: &str,
    numeric: &str,
    locale: &str,
) -> Result<Value, VmError> {
    if !value.is_finite() {
        return Err(runtime_error("RangeError: value must be finite"));
    }
    parts_value(value, unit, style, numeric, locale)
}

fn relative_resolved_options(
    slots: &[(String, Value)],
    locale: String,
    style: String,
    numeric: String,
) -> Result<Value, VmError> {
    Ok(make_object(vec![
        ("locale".to_string(), Value::String(locale)),
        ("style".to_string(), Value::String(style)),
        ("numeric".to_string(), Value::String(numeric)),
        (
            "numberingSystem".to_string(),
            Value::String(
                slot_string(slots, "numberingSystem").unwrap_or_else(|| "latn".to_string()),
            ),
        ),
    ]))
}
