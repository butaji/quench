fn function_name_is_unset(function: &crate::value::FunctionValue) -> bool {
    !function
        .properties
        .borrow()
        .iter()
        .any(|(name, _)| name == "name")
}

fn define_function_name(function: &crate::value::FunctionValue, value: Value) {
    let descriptor = Value::Object(Rc::new(vec![
        ("value".to_string(), value.clone()),
        ("writable".to_string(), Value::Boolean(false)),
        ("enumerable".to_string(), Value::Boolean(false)),
        ("configurable".to_string(), Value::Boolean(true)),
    ]));
    let mut properties = function.properties.borrow_mut();
    properties.push(("name".to_string(), value));
    properties.push((descriptor_key("name"), descriptor));
}
