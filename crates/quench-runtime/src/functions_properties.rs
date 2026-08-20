fn function_properties(length: u16) -> std::rc::Rc<std::cell::RefCell<Vec<(String, crate::value::Value)>>> {
    let name = crate::value::Value::String(String::new());
    let name_descriptor = crate::value::Value::Object(std::rc::Rc::new(crate::value::ObjectData::new(vec![
        ("value".to_string(), name.clone()),
        ("writable".to_string(), crate::value::Value::Boolean(false)),
        ("enumerable".to_string(), crate::value::Value::Boolean(false)),
        ("configurable".to_string(), crate::value::Value::Boolean(true)),
    ])));
    let value = crate::value::Value::Number(f64::from(length));
    let descriptor = crate::value::Value::Object(std::rc::Rc::new(crate::value::ObjectData::new(vec![
        ("value".to_string(), value.clone()),
        ("writable".to_string(), crate::value::Value::Boolean(false)),
        ("enumerable".to_string(), crate::value::Value::Boolean(false)),
        ("configurable".to_string(), crate::value::Value::Boolean(true)),
    ])));
    let realm = crate::vm::current_context_or_default().realm();
    let mut properties = vec![
        ("length".to_string(), value),
        (crate::builtins::descriptor_key("length"), descriptor),
        ("name".to_string(), name),
        (crate::builtins::descriptor_key("name"), name_descriptor),
    ];
    if let Some(token) = crate::vm::realm_token(realm) {
        properties.push(("\0realm".to_string(), token));
    }
    std::rc::Rc::new(std::cell::RefCell::new(properties))
}
