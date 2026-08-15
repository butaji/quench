fn receiver_slots(receiver: Option<&Value>) -> Result<Vec<(String, Value)>, VmError> {
    super::intl_slots(receiver)
}

pub(crate) fn dispatch(
    builtin: crate::ops::Builtin,
    arguments: &[Value],
    receiver: Option<&Value>,
) -> Option<Result<Value, VmError>> {
    match builtin {
        crate::ops::Builtin::IntlRelativeTimeFormat => Some(construct(arguments)),
        crate::ops::Builtin::IntlRelativeTimeFormatFormat
        | crate::ops::Builtin::IntlRelativeTimeFormatFormatToParts
        | crate::ops::Builtin::IntlRelativeTimeFormatResolvedOptions => {
            Some(prototype_method(builtin, arguments, receiver))
        }
        _ => None,
    }
}
