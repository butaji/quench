fn receiver_slots(receiver: Option<&Value>) -> Result<Vec<(String, Value)>, VmError> {
    super::intl_slots(receiver)
}

pub(crate) fn dispatch(
    builtin: crate::ops::Builtin,
    arguments: &[Value],
    receiver: Option<&Value>,
) -> Option<Result<Value, VmError>> {
    match builtin {
        crate::ops::Builtin::IntlDateTimeFormat => Some(construct(arguments)),
        crate::ops::Builtin::IntlDateTimeFormatFormatGetter => Some(format_getter(receiver)),
        crate::ops::Builtin::IntlDateTimeFormatFormat
        | crate::ops::Builtin::IntlDateTimeFormatFormatToParts
        | crate::ops::Builtin::IntlDateTimeFormatFormatRange
        | crate::ops::Builtin::IntlDateTimeFormatFormatRangeToParts
        | crate::ops::Builtin::IntlDateTimeFormatResolvedOptions => {
            Some(prototype_method(builtin, arguments, receiver))
        }
        _ => None,
    }
}

fn format_getter(receiver: Option<&Value>) -> Result<Value, VmError> {
    let receiver = receiver.ok_or_else(|| runtime_error("TypeError: not an Intl object"))?;
    if !matches!(receiver, Value::Object(properties) if properties.iter().any(|(name, _)| name == super::datetime::SLOT))
    {
        return Err(runtime_error("TypeError: not an Intl object"));
    }
    Ok(crate::vm::bind_receiver_property(
        Value::Builtin(crate::ops::Builtin::IntlDateTimeFormatFormat),
        receiver,
    ))
}
