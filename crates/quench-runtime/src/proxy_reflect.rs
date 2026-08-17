fn reflect_define_property(arguments: &[Value]) -> Result<Value, VmError> {
    let target = reflect_target(arguments)?;
    let prop = reflect_property(arguments)?;
    let descriptor = arguments.get(2).cloned().unwrap_or(Value::Undefined);
    if !crate::value::is_object(&descriptor) {
        return Err(crate::value::error::throw_type_error(
            "Property description must be an object",
        ));
    }
    let descriptor = to_property_descriptor(&descriptor)?;
    match proxy_define_property(target, &prop, &descriptor) {
        Ok(_) => Ok(Value::Boolean(true)),
        Err(error) if define_rejected(&error) => Ok(Value::Boolean(false)),
        Err(error) => Err(error),
    }
}

fn to_property_descriptor(descriptor: &Value) -> Result<Value, VmError> {
    let Value::Object(properties) = descriptor else {
        return Ok(descriptor.clone());
    };
    const FIELDS: [&str; 6] = ["enumerable", "configurable", "value", "writable", "get", "set"];
    let mut resolved = Vec::new();
    for field in FIELDS {
        if properties.iter().any(|(name, _)| name == field) {
            let value = crate::vm::get_property_result(descriptor, field)?;
            resolved.push((field.to_string(), value));
        }
    }
    Ok(Value::Object(std::rc::Rc::new(
        crate::value::ObjectData::new(resolved),
    )))
}

fn define_rejected(error: &VmError) -> bool {
    let VmError::Thrown(Value::Object(properties)) = error else {
        return false;
    };
    let message = properties.iter().find_map(|(name, value)| match (name.as_str(), value) {
        ("message", Value::String(text)) => Some(text.as_str()),
        _ => None,
    });
    match message {
        Some("Cannot define a property on a non-extensible object") => true,
        Some(text) => text.starts_with("Cannot redefine"),
        None => false,
    }
}

fn reflect_own_keys(arguments: &[Value]) -> Result<Value, VmError> {
    proxy_own_keys(arguments.first().ok_or(VmError::NotCallable)?)
}

fn extract_array_arg(arguments: &[Value], index: usize) -> Result<Vec<Value>, VmError> {
    let value = arguments.get(index).ok_or_else(|| {
        crate::value::error::throw_type_error("Reflect arguments list must be an object")
    })?;
    if !crate::value::is_object(value) {
        return Err(crate::value::error::throw_type_error(
            "Reflect arguments list must be an object",
        ));
    }
    crate::vm::create_list_from_array_like(Some(value))
}

fn reflect_apply(arguments: &[Value]) -> Result<Value, VmError> {
    let target = arguments.first().ok_or(VmError::NotCallable)?;
    let this_arg = arguments.get(1).cloned().unwrap_or(Value::Undefined);
    proxy_apply(target, &this_arg, &extract_array_arg(arguments, 2)?)
}

fn reflect_construct(arguments: &[Value]) -> Result<Value, VmError> {
    let target = arguments.first().ok_or(VmError::NotCallable)?;
    proxy_construct(target, &extract_array_arg(arguments, 1)?, arguments.get(2))
}
fn reflect_get(arguments: &[Value]) -> Result<Value, VmError> {
    let target = reflect_target(arguments)?;
    let prop = reflect_property(arguments)?;
    proxy_get(target, &prop, arguments.get(2))
}

fn reflect_set(arguments: &[Value]) -> Result<Value, VmError> {
    let target = reflect_target(arguments)?;
    let prop = reflect_property(arguments)?;
    let value = arguments.get(2).cloned().unwrap_or(Value::Undefined);
    proxy_set(target, &prop, &value, arguments.get(3))
}

fn reflect_has(arguments: &[Value]) -> Result<Value, VmError> {
    let target = reflect_target(arguments)?;
    proxy_has(target, &reflect_property(arguments)?)
}

fn reflect_delete_property(arguments: &[Value]) -> Result<Value, VmError> {
    let target = reflect_target(arguments)?;
    proxy_delete(target, &reflect_property(arguments)?)
}

fn reflect_get_prototype_of(arguments: &[Value]) -> Result<Value, VmError> {
    proxy_get_prototype_of(reflect_target(arguments)?)
}

fn reflect_set_prototype_of(arguments: &[Value]) -> Result<Value, VmError> {
    let target = reflect_target(arguments)?;
    proxy_set_prototype_of(target, &arguments.get(1).cloned().unwrap_or(Value::Null))
}

fn reflect_is_extensible(arguments: &[Value]) -> Result<Value, VmError> {
    proxy_is_extensible(reflect_target(arguments)?)
}

fn reflect_prevent_extensions(arguments: &[Value]) -> Result<Value, VmError> {
    proxy_prevent_extensions(reflect_target(arguments)?)
}

fn reflect_get_own_property_descriptor(arguments: &[Value]) -> Result<Value, VmError> {
    let target = reflect_target(arguments)?;
    proxy_get_own_property_descriptor(target, &reflect_property(arguments)?)
}

fn reflect_target(arguments: &[Value]) -> Result<&Value, VmError> {
    let target = arguments.first().ok_or(VmError::NotCallable)?;
    require_reflect_object(target)?;
    Ok(target)
}

fn reflect_property(arguments: &[Value]) -> Result<String, VmError> {
    crate::conversion::to_property_key(arguments.get(1).unwrap_or(&Value::Undefined))
}
pub(crate) fn proxy_own_keys(target: &Value) -> Result<Value, VmError> {
    if let Value::Proxy(proxy) = target {
        check_revoked(proxy)?;
        if let Some(trap) = get_handler_trap(proxy, "ownKeys") {
            let result = call_trap(
                &trap,
                std::slice::from_ref(&proxy.target),
                Some(&proxy.handler),
            )?;
            validate_own_keys_result(&result)?;
            return Ok(result);
        }
    }
    let target = match target {
        Value::Proxy(proxy) => &proxy.target,
        target => target,
    };
    if matches!(target, Value::Proxy(_)) {
        proxy_own_keys(target)
    } else {
        crate::own_keys::all(target)
    }
}

fn validate_own_keys_result(value: &Value) -> Result<(), VmError> {
    let Value::Array(keys) = value else {
        return Err(crate::value::error::throw_type_error(
            "Proxy ownKeys trap must return an object",
        ));
    };
    let mut seen = Vec::new();
    for key in keys.snapshot() {
        let Value::String(key) = key else {
            return Err(crate::value::error::throw_type_error(
                "Proxy ownKeys trap returned a non-property key",
            ));
        };
        if seen.contains(&key) {
            return Err(crate::value::error::throw_type_error(
                "Proxy ownKeys trap returned duplicate keys",
            ));
        }
        seen.push(key);
    }
    Ok(())
}
fn descriptor_value<'a>(descriptor: &'a Value, field: &str) -> Option<&'a Value> {
    let Value::Object(properties) = descriptor else {
        return None;
    };
    properties
        .iter()
        .rev()
        .find_map(|(name, value)| (name == field).then_some(value))
}
