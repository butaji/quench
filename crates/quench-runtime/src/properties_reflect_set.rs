pub(crate) fn set_with_receiver(
    target: &crate::value::Value,
    key: &str,
    value: &crate::value::Value,
    receiver: &crate::value::Value,
) -> Result<bool, crate::execute::VmError> {
    if !crate::value::is_object(receiver) {
        return Ok(false);
    }
    let descriptor = inherited_descriptor(target, key)?;
    match descriptor {
        Some(descriptor) if descriptor_field_exists(&descriptor, "set") => {
            call_setter(&descriptor, receiver, value)
        }
        Some(descriptor) if !descriptor_writable(&descriptor)? => Ok(false),
        _ => set_receiver_data(receiver, key, value),
    }
}

fn inherited_descriptor(
    target: &crate::value::Value,
    key: &str,
) -> Result<Option<crate::value::Value>, crate::execute::VmError> {
    let mut current = target.clone();
    loop {
        let descriptor = crate::builtins::object::descriptor(
            Some(&current),
            Some(&crate::value::Value::String(key.to_string())),
        )?;
        if !matches!(descriptor, crate::value::Value::Undefined) {
            return Ok(Some(descriptor));
        }
        current = crate::builtins::object::get_prototype_of(Some(&current))?;
        if matches!(current, crate::value::Value::Null) {
            return Ok(None);
        }
    }
}

fn descriptor_field_exists(descriptor: &crate::value::Value, field: &str) -> bool {
    !matches!(
        crate::builtins::object::descriptor(
            Some(descriptor),
            Some(&crate::value::Value::String(field.to_string())),
        ),
        Ok(crate::value::Value::Undefined)
    )
}

fn descriptor_writable(descriptor: &crate::value::Value) -> Result<bool, crate::execute::VmError> {
    Ok(matches!(
        crate::execute::get_property_result(descriptor, "writable")?,
        crate::value::Value::Boolean(true)
    ))
}

fn call_setter(
    descriptor: &crate::value::Value,
    receiver: &crate::value::Value,
    value: &crate::value::Value,
) -> Result<bool, crate::execute::VmError> {
    let setter = crate::execute::get_property_result(descriptor, "set")?;
    if matches!(setter, crate::value::Value::Undefined) {
        return Ok(false);
    }
    crate::functions::execute_target_with_receiver(&setter, receiver, std::slice::from_ref(value))?;
    Ok(true)
}

fn set_receiver_data(
    receiver: &crate::value::Value,
    key: &str,
    value: &crate::value::Value,
) -> Result<bool, crate::execute::VmError> {
    let current = crate::builtins::object::descriptor(
        Some(receiver),
        Some(&crate::value::Value::String(key.to_string())),
    )?;
    if !matches!(current, crate::value::Value::Undefined) {
        if descriptor_field_exists(&current, "set") || !descriptor_writable(&current)? {
            return Ok(false);
        }
    } else if rejects_new_property(receiver, key) {
        return Ok(false);
    }
    let descriptor = receiver_data_descriptor(value, matches!(current, crate::value::Value::Undefined));
    let updated = crate::builtins::define_own_property(receiver, key, &descriptor)?;
    crate::locals::replace_value(receiver, &updated);
    Ok(true)
}

fn receiver_data_descriptor(
    value: &crate::value::Value,
    create: bool,
) -> Vec<(String, crate::value::Value)> {
    let mut descriptor = vec![("value".to_string(), value.clone())];
    if create {
        descriptor.extend([
            ("writable".to_string(), crate::value::Value::Boolean(true)),
            ("enumerable".to_string(), crate::value::Value::Boolean(true)),
            ("configurable".to_string(), crate::value::Value::Boolean(true)),
        ]);
    }
    descriptor
}
