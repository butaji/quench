pub(crate) fn descriptor_flag(target: &Value, key: &str, field: &str) -> Option<bool> {
    if crate::vm::is_global_object(target)
        && field == "writable"
        && matches!(key, "undefined" | "Infinity" | "NaN")
    {
        return Some(false);
    }
    match target {
        Value::Object(properties) => descriptor_flag_in(properties, key, field),
        Value::ObjectAlias(alias) => alias
            .0
            .borrow()
            .upgrade()
            .and_then(|properties| descriptor_flag_in(&properties, key, field)),
        Value::Array(values) => array_flag(values, key, field),
        Value::Builtin(builtin) => builtin_flag(*builtin, key, field),
        _ => None,
    }
}

fn builtin_flag(builtin: crate::ops::Builtin, key: &str, field: &str) -> Option<bool> {
    let descriptor = crate::builtins::object::descriptor(
        Some(&Value::Builtin(builtin)),
        Some(&Value::String(key.to_string())),
    )
    .ok()?;
    let Value::Object(properties) = descriptor else {
        return None;
    };
    properties
        .iter()
        .rev()
        .find_map(|(name, value)| (name == field).then_some(matches!(value, Value::Boolean(true))))
}

fn array_flag(values: &crate::value::ArrayData, key: &str, field: &str) -> Option<bool> {
    let Some(descriptor) = values.descriptor(key) else {
        return argument_property_flag(values, key, field);
    };
    let Value::Object(descriptor) = descriptor else {
        return None;
    };
    descriptor
        .iter()
        .rev()
        .find_map(|(name, value)| (name == field).then_some(matches!(value, Value::Boolean(true))))
}

fn argument_property_flag(
    values: &crate::value::ArrayData,
    key: &str,
    field: &str,
) -> Option<bool> {
    if !values.is_arguments() || !matches!(key, "length" | "callee" | "Symbol.iterator") {
        return None;
    }
    Some(match field {
        "enumerable" => false,
        "configurable" => !values.is_strict_arguments() || key != "callee",
        "writable" => !values.is_strict_arguments() || key != "callee",
        _ => return None,
    })
}

fn descriptor_flag_in(properties: &[(String, Value)], key: &str, field: &str) -> Option<bool> {
    let metadata_key = descriptor_key(key);
    let (_, Value::Object(descriptor)) = properties
        .iter()
        .rev()
        .find(|(name, _)| name == &metadata_key)?
    else {
        return None;
    };
    descriptor
        .iter()
        .rev()
        .find_map(|(name, value)| (name == field).then_some(matches!(value, Value::Boolean(true))))
}

fn complete_descriptor(descriptor: &[(String, Value)], current: &Value) -> Vec<(String, Value)> {
    let requested_accessor = descriptor
        .iter()
        .any(|(name, _)| matches!(name.as_str(), "get" | "set"));
    let requested_data = descriptor
        .iter()
        .any(|(name, _)| matches!(name.as_str(), "value" | "writable"));
    let current_accessor =
        descriptor_value(current, "get").is_some() || descriptor_value(current, "set").is_some();
    let accessor = requested_accessor || (!requested_data && current_accessor);
    let fields = if accessor {
        ["get", "set", "enumerable", "configurable"]
    } else {
        ["value", "writable", "enumerable", "configurable"]
    };
    fields
        .into_iter()
        .map(|name| {
            let default = descriptor_default(current, name);
            let value = descriptor
                .iter()
                .rev()
                .find(|(field, _)| field == name)
                .map_or(default, |(_, value)| complete_descriptor_value(name, value));
            (name.to_string(), value)
        })
        .collect()
}

fn complete_descriptor_value(name: &str, value: &Value) -> Value {
    if matches!(name, "writable" | "enumerable" | "configurable") {
        return Value::Boolean(crate::execute::is_truthy(value));
    }
    value.clone()
}

fn descriptor_default(current: &Value, name: &str) -> Value {
    if let Value::Object(properties) = current {
        if let Some((_, value)) = properties.iter().rev().find(|(field, _)| field == name) {
            return value.clone();
        }
    }
    match name {
        "writable" | "enumerable" | "configurable" => Value::Boolean(false),
        _ => Value::Undefined,
    }
}

fn validate_redefinition(
    current: &Value,
    requested: &[(String, Value)],
) -> Result<(), crate::execute::VmError> {
    if descriptor_value(current, "configurable") != Some(&Value::Boolean(false)) {
        return Ok(());
    }
    if descriptor_value_in(requested, "configurable") == Some(&Value::Boolean(true))
        || changes_descriptor_kind(current, requested)
    {
        return Err(cannot_redefine());
    }
    if descriptor_value(current, "writable") == Some(&Value::Boolean(false))
        && descriptor_value_in(requested, "writable") == Some(&Value::Boolean(true))
    {
        return Err(cannot_redefine());
    }
    if descriptor_value(current, "writable") == Some(&Value::Boolean(false))
        && descriptor_value_in(requested, "value").is_some_and(|requested_value| {
            !crate::builtins::same_value(Some(requested_value), descriptor_value(current, "value"))
        })
    {
        return Err(cannot_redefine());
    }
    Ok(())
}

fn changes_descriptor_kind(current: &Value, requested: &[(String, Value)]) -> bool {
    let current_accessor =
        descriptor_value(current, "get").is_some() || descriptor_value(current, "set").is_some();
    let requests_accessor = descriptor_value_in(requested, "get").is_some()
        || descriptor_value_in(requested, "set").is_some();
    let requests_data = descriptor_value_in(requested, "value").is_some()
        || descriptor_value_in(requested, "writable").is_some();
    current_accessor && requests_data || !current_accessor && requests_accessor
}

fn descriptor_value<'a>(descriptor: &'a Value, field: &str) -> Option<&'a Value> {
    let Value::Object(properties) = descriptor else {
        return None;
    };
    descriptor_value_in(properties, field)
}

fn descriptor_value_in<'a>(descriptor: &'a [(String, Value)], field: &str) -> Option<&'a Value> {
    descriptor
        .iter()
        .rev()
        .find_map(|(name, value)| (name == field).then_some(value))
}

fn cannot_redefine() -> crate::execute::VmError {
    crate::value::error::throw_type_error("Cannot redefine non-configurable property")
}
