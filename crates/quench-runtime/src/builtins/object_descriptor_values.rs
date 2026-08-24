fn string_descriptor(value: &str, key: &str) -> Option<Value> {
    if key == "length" {
        return Some(string_length_descriptor(value));
    }
    crate::strings::char_at_utf16(value, key.parse::<usize>().ok()?)
        .map(|character| descriptor_object_with_flags(character, false, true, false))
}

fn string_length_descriptor(value: &str) -> Value {
    descriptor_object_with_flags(
        Value::Number(crate::strings::utf16_len(value) as f64),
        false,
        false,
        false,
    )
}

fn descriptor_object(value: &Value) -> Value {
    descriptor_object_with_flags(public_value(value), true, true, true)
}

fn live_descriptor(descriptor: &Value, live: Option<Value>) -> Value {
    let Value::Object(properties) = public_descriptor(descriptor) else {
        return public_descriptor(descriptor);
    };
    let Some(live) = live else {
        return Value::Object(properties);
    };
    let mut properties = properties.properties.clone();
    if let Some((_, value)) = properties.iter_mut().find(|(name, _)| name == "value") {
        *value = live;
    }
    Value::Object(Rc::new(ObjectData::from_shared_properties(properties)))
}

fn public_descriptor(descriptor: &Value) -> Value {
    let Value::Object(properties) = descriptor else {
        return descriptor.clone();
    };
    let mut properties = properties.properties.clone();
    if let Some((_, value)) = properties.iter_mut().find(|(name, _)| name == "value") {
        *value = public_value(value);
    }
    Value::Object(Rc::new(ObjectData::from_shared_properties(properties)))
}

fn public_value(value: &Value) -> Value {
    match value {
        Value::BindingCell(cell) => public_value(&cell.borrow()),
        Value::WeakFunction(function) => function.value(),
        value => value.clone(),
    }
}

fn descriptor_object_with_flags(
    value: Value,
    writable: bool,
    enumerable: bool,
    configurable: bool,
) -> Value {
    crate::execution_trace::descriptor_object("view");
    Value::Object(Rc::new(ObjectData::new(vec![
        ("value".to_string(), public_value(&value)),
        ("writable".to_string(), Value::Boolean(writable)),
        ("enumerable".to_string(), Value::Boolean(enumerable)),
        ("configurable".to_string(), Value::Boolean(configurable)),
    ])))
}
