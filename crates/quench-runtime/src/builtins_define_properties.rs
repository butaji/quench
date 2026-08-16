pub(crate) fn define_properties(arguments: &[Value]) -> Result<Value, crate::execute::VmError> {
    let target = arguments.first().cloned().unwrap_or(Value::Undefined);
    if !crate::value::is_object(&target) {
        return Err(crate::value::error::throw_type_error(
            "Property definition target must be an object",
        ));
    }
    let Some(properties) = arguments.get(1) else {
        return Err(crate::value::error::throw_type_error(
            "Property descriptors must be an object",
        ));
    };
    let properties = crate::construct::to_object(properties)?;
    let keys = crate::proxy::proxy_own_keys(&properties)?;
    let Value::Array(keys) = keys else {
        return Ok(target);
    };
    keys.iter().try_fold(target, |target, key| {
        let key = crate::conversion::to_property_key(key)?;
        let descriptor = property_descriptor(&properties, &key)?;
        let Some(descriptor) = descriptor_object(descriptor) else {
            return Ok(target);
        };
        let descriptor = descriptor_fields(&descriptor)?;
        let result = define_own_property(&target, &key, &descriptor)?;
        crate::locals::replace_value(&target, &result);
        Ok(result)
    })
}

fn property_descriptor(properties: &Value, key: &str) -> Result<Value, crate::execute::VmError> {
    if matches!(properties, Value::Proxy(_)) {
        let property = crate::proxy::proxy_get_own_property_descriptor(properties, key)?;
        if !property_is_enumerable(&property) {
            return Ok(Value::Undefined);
        }
        if matches!(property, Value::Undefined) {
            return Ok(Value::Undefined);
        }
        return crate::execute::get_property_result(&property, "value");
    }
    if crate::builtins::descriptor_flag(properties, key, "enumerable") == Some(false) {
        return Ok(Value::Undefined);
    }
    crate::execute::get_property_result(properties, key)
}

fn property_is_enumerable(property: &Value) -> bool {
    let Value::Object(properties) = property else {
        return true;
    };
    !properties
        .iter()
        .rev()
        .any(|(name, value)| name == "enumerable" && matches!(value, Value::Boolean(false)))
}

fn descriptor_object(value: Value) -> Option<Value> {
    crate::value::is_object(&value).then_some(value)
}

pub(crate) fn descriptor_fields(
    descriptor: &Value,
) -> Result<Vec<(String, Value)>, crate::execute::VmError> {
    let mut fields = Vec::new();
    for field in [
        "get",
        "set",
        "value",
        "writable",
        "enumerable",
        "configurable",
    ] {
        if !crate::with_scope::has_property(descriptor, field)? {
            continue;
        }
        let value = crate::execute::get_property_result(descriptor, field)?;
        let value = if matches!(field, "writable" | "enumerable" | "configurable") {
            Value::Boolean(crate::execute::is_truthy(&value))
        } else {
            value
        };
        fields.push((field.to_string(), value));
    }
    validate_accessor_fields(&fields)?;
    Ok(fields)
}

fn validate_accessor_fields(fields: &[(String, Value)]) -> Result<(), crate::execute::VmError> {
    for name in ["get", "set"] {
        let Some((_, value)) = fields.iter().find(|(field, _)| field == name) else {
            continue;
        };
        if !matches!(value, Value::Undefined) && !crate::conversion::is_callable(value) {
            return Err(crate::value::error::throw_type_error(
                "Accessor descriptor must be callable",
            ));
        }
    }
    Ok(())
}
