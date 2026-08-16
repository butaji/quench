fn receiver_slots(receiver: Option<&Value>) -> Result<Vec<(String, Value)>, VmError> {
    super::intl_slots(receiver)
}

pub(crate) fn dispatch(
    builtin: crate::ops::Builtin,
    arguments: &[Value],
    receiver: Option<&Value>,
) -> Option<Result<Value, VmError>> {
    match builtin {
        crate::ops::Builtin::IntlDateTimeFormat => Some(construct_call(arguments, receiver)),
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

fn construct_call(arguments: &[Value], receiver: Option<&Value>) -> Result<Value, VmError> {
    let Some(receiver) = receiver else {
        return construct(arguments);
    };
    let legacy_realm = legacy_receiver_realm(receiver)?;
    if legacy_realm.is_some() && receiver_slots(Some(receiver)).is_ok() {
        return Ok(receiver.clone());
    }
    let Some(realm) = legacy_realm else {
        return construct(arguments);
    };
    let initialized = construct(arguments)?;
    let slots = crate::execute::get_property(&initialized, super::datetime::SLOT);
    let symbol = crate::vm::intl_fallback_symbol(realm)
        .ok_or_else(|| runtime_error("TypeError: missing Intl fallback symbol"))?;
    let receiver = crate::builtins::set_property(receiver.clone(), super::datetime::SLOT, slots);
    let key = symbol_key(&symbol)?;
    Ok(crate::builtins::set_property(receiver, &key, symbol))
}

fn legacy_receiver_realm(receiver: &Value) -> Result<Option<crate::ops::RealmId>, VmError> {
    let mut prototype = crate::builtins::object::get_prototype_of(Some(receiver))?;
    while !matches!(prototype, Value::Null) {
        if let Some(realm) =
            crate::vm::intrinsic_realm(&prototype, crate::ops::Builtin::IntlDateTimeFormatPrototype)
        {
            return Ok(Some(realm));
        }
        prototype = crate::builtins::object::get_prototype_of(Some(&prototype))?;
    }
    Ok(None)
}

fn symbol_key(symbol: &Value) -> Result<String, VmError> {
    match symbol {
        Value::String(value) => Ok(value.clone()),
        _ => Err(runtime_error("TypeError: invalid fallback symbol")),
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
