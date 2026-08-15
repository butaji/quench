pub(crate) fn define_properties(arguments: &[Value]) -> Result<Value, crate::execute::VmError> {
    let target = arguments.first().cloned().unwrap_or(Value::Undefined);
    let Some(properties) = arguments.get(1) else {
        return Err(crate::value::error::throw_type_error(
            "Property descriptors must be an object",
        ));
    };
    if !crate::value::is_object(properties) {
        return Err(crate::value::error::throw_type_error(
            "Property descriptors must be an object",
        ));
    }
    let keys = crate::own_keys::keys_result(Some(properties))?;
    let Value::Array(keys) = keys else {
        return Ok(target);
    };
    keys.iter().try_fold(target, |target, key| {
            let key = crate::conversion::to_property_key(key)?;
            let descriptor = crate::execute::get_property_result(properties, &key)?;
            let Some(descriptor) = descriptor_object(descriptor) else {
                return Err(crate::value::error::throw_type_error(
                    "Property descriptor must be an object",
                ));
            };
            let descriptor = descriptor_fields(&descriptor)?;
            define_own_property(&target, &key, &descriptor)
        })
}

fn descriptor_object(value: Value) -> Option<Value> {
    match value {
        Value::Object(_) | Value::ObjectAlias(_) => Some(value),
        _ => None,
    }
}

fn descriptor_fields(
    descriptor: &Value,
) -> Result<Vec<(String, Value)>, crate::execute::VmError> {
    ["get", "set", "value", "writable", "enumerable", "configurable"]
        .into_iter()
        .filter(|field| {
            let key = Value::String(field.to_string());
            crate::builtins::object::descriptor(Some(descriptor), Some(&key))
                .ok()
                .is_some_and(|value| !matches!(value, Value::Undefined))
        })
        .map(|field| {
            crate::execute::get_property_result(descriptor, field)
                .map(|value| (field.to_string(), value))
        })
        .collect()
}
