pub(crate) fn set_function_name(
    value: &Value,
    name: &str,
) -> Result<(), crate::execute::VmError> {
    let Value::Function(function) = value else {
        return Err(crate::execute::VmError::MissingReturn);
    };
    define_function_name(function, Value::String(name.to_string()));
    Ok(())
}

fn define_function_name(function: &crate::value::FunctionValue, value: Value) {
    let descriptor = Value::Object(Rc::new(vec![
        ("value".to_string(), value.clone()),
        ("writable".to_string(), Value::Boolean(false)),
        ("enumerable".to_string(), Value::Boolean(false)),
        ("configurable".to_string(), Value::Boolean(true)),
    ]));
    let mut properties = function.properties.borrow_mut();
    properties.retain(|(name, _)| name != "name" && name != &descriptor_key("name"));
    properties.push(("name".to_string(), value));
    properties.push((descriptor_key("name"), descriptor));
}
