pub(crate) fn descriptor_flag(target: &Value, key: &str, field: &str) -> Option<bool> {
    match target {
        Value::Object(properties) => descriptor_flag_in(properties, key, field),
        Value::Array(values) => array_flag(values, key, field),
        _ => None,
    }
}

fn array_flag(values: &crate::value::ArrayData, key: &str, field: &str) -> Option<bool> {
    let Value::Object(descriptor) = values.descriptor(key)? else {
        return None;
    };
    descriptor
        .iter()
        .rev()
        .find_map(|(name, value)| (name == field).then_some(matches!(value, Value::Boolean(true))))
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
    let accessor = descriptor
        .iter()
        .any(|(name, _)| matches!(name.as_str(), "get" | "set"));
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
                .map_or(default, |(_, value)| value.clone());
            (name.to_string(), value)
        })
        .collect()
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
