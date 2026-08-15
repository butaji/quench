fn boxed_string_immutable_key(properties: &ObjectData, key: &str) -> bool {
    let is_string = properties
        .iter()
        .any(|(name, value)| name == "_value" && matches!(value, Value::String(_)));
    is_string && (key == "length" || key.parse::<usize>().is_ok())
}
