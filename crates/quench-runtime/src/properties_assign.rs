pub(crate) fn assign_set_property(
    target: &crate::value::Value,
    key: &str,
    value: crate::value::Value,
) -> Result<crate::value::Value, crate::execute::VmError> {
    reject_nullish_property_write(target)?;
    if let crate::value::Value::Proxy(_) = target {
        let result = crate::proxy::proxy_set(target, key, &value, Some(target))?;
        if matches!(result, crate::value::Value::Boolean(false)) {
            return Err(crate::value::error::throw_type_error(
                "Proxy set trap returned false",
            ));
        }
        return Ok(target.clone());
    }
    if crate::typed_array_ops::is_view(target)
        && crate::typed_array_ops::is_index_key(key)
    {
        if let Some(result) = crate::typed_array_ops::set_property(target, key, &value) {
            return Ok(result.unwrap_or_else(|_| target.clone()));
        }
    }
    if let crate::value::Value::Object(properties) = target {
        if crate::builtins::boxed_string_immutable_key(properties, key) {
            return Err(crate::value::error::throw_type_error(
                "Cannot assign to read-only property",
            ));
        }
    }
    let own_readonly = crate::builtins::object::descriptor(
        Some(target),
        Some(&crate::value::Value::String(key.to_string())),
    )
    .ok()
    .is_some_and(|descriptor| {
        matches!(descriptor, crate::value::Value::Object(properties) if properties.iter().any(|(name, value)| name == "writable" && matches!(value, crate::value::Value::Boolean(false))))
    });
    if own_readonly {
        return Err(crate::value::error::throw_type_error(
            "Cannot assign to read-only property",
        ));
    }
    if rejects_new_property(target, key) || inherited_write_blocked(target, key) {
        return Err(crate::value::error::throw_type_error(
            "Cannot assign to read-only property",
        ));
    }
    if let Some(setter) = crate::property_define::accessor(target, key, "set") {
        if matches!(setter, crate::value::Value::Undefined) {
            return Err(crate::value::error::throw_type_error(
                "Cannot set property without a setter",
            ));
        }
        let (_, _) = crate::functions::execute_target_with_receiver(
            &setter,
            target,
            std::slice::from_ref(&value),
        )?;
        return Ok(crate::locals::resolved_replacement(target.clone()));
    }
    Ok(crate::builtins::set_property(target.clone(), key, value))
}

fn assign_proxy_set(
    registers: &mut crate::register_file::RegisterFile,
    object: u16,
    target: &crate::value::Value,
    key: &str,
    value: crate::value::Value,
) -> Result<(), crate::execute::VmError> {
    let result = assign_set_property(target, key, value)?;
    crate::execute::write_value(registers, object, result);
    Ok(())
}

fn delete_proxy_property(
    registers: &mut crate::register_file::RegisterFile,
    dst: u16,
    target: &crate::value::Value,
    key: &str,
    strict: bool,
) -> Result<(), crate::execute::VmError> {
    let result = crate::proxy::proxy_delete(target, key)?;
    let deleted = matches!(result, crate::value::Value::Boolean(true));
    if !deleted && strict {
        return Err(crate::value::error::throw_type_error(
            "Cannot delete property through Proxy",
        ));
    }
    crate::execute::write_value(registers, dst, result);
    Ok(())
}
