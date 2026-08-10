pub(crate) fn set_function_name(value: &Value, name: &str) -> Result<(), crate::execute::VmError> {
    let Value::Function(function) = value else {
        return Err(crate::execute::VmError::MissingReturn);
    };
    define_function_name(function, Value::String(name.to_string()));
    Ok(())
}

pub(crate) fn set_dynamic_function_name(
    value: &Value,
    key: &Value,
    prefix: Option<&str>,
) -> Result<(), crate::execute::VmError> {
    let name = dynamic_function_name(key, prefix)?;
    set_function_name(value, &name)
}

fn dynamic_function_name(
    key: &Value,
    prefix: Option<&str>,
) -> Result<String, crate::execute::VmError> {
    let key = function_name_key(key)?;
    Ok(prefix.map_or(key.clone(), |prefix| format!("{prefix} {key}")))
}

fn function_name_key(key: &Value) -> Result<String, crate::execute::VmError> {
    if crate::conversion::is_symbol(key) {
        return Ok(format!("[{}]", symbol_description(key)));
    }
    crate::conversion::to_property_key(key)
}

fn symbol_description(key: &Value) -> String {
    let Value::String(key) = key else {
        return match key {
            Value::Builtin(builtin) => crate::intl::tolocale::symbol::name(*builtin)
                .map_or_else(String::new, str::to_string),
            _ => String::new(),
        };
    };
    let description = key
        .strip_prefix("Symbol.for.")
        .or_else(|| key.strip_prefix("Symbol."))
        .map_or(key.as_str(), |description| description);
    description
        .split('\0')
        .next()
        .map_or_else(String::new, str::to_string)
}

fn define_function_name(function: &crate::value::FunctionValue, value: Value) {
    let descriptor = Value::Object(Rc::new(crate::value::ObjectData::new(vec![
        ("value".to_string(), value.clone()),
        ("writable".to_string(), Value::Boolean(false)),
        ("enumerable".to_string(), Value::Boolean(false)),
        ("configurable".to_string(), Value::Boolean(true)),
    ])));
    let mut properties = function.properties.borrow_mut();
    properties.retain(|(name, _)| name != "name" && name != &descriptor_key("name"));
    let index = properties
        .iter()
        .position(|(name, _)| name == "prototype")
        .unwrap_or(properties.len());
    properties.insert(index, ("name".to_string(), value));
    properties.insert(index + 1, (descriptor_key("name"), descriptor));
}
