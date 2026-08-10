pub(crate) fn descriptor_flag(target: &Value, key: &str, field: &str) -> Option<bool> {
    let Value::Object(properties) = target else {
        return None;
    };
    descriptor_flag_in(properties, key, field)
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

fn complete_descriptor(descriptor: &[(String, Value)]) -> Vec<(String, Value)> {
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
            let default = match name {
                "writable" | "enumerable" | "configurable" => Value::Boolean(false),
                _ => Value::Undefined,
            };
            let value = descriptor
                .iter()
                .rev()
                .find(|(field, _)| field == name)
                .map_or(default, |(_, value)| value.clone());
            (name.to_string(), value)
        })
        .collect()
}
