pub(crate) fn delete_property(target: Value, key: &str) -> (Value, bool) {
    match target {
        Value::Object(properties)
            if descriptor_flag_in(&properties, key, "configurable") == Some(false) =>
        {
            (Value::Object(properties), false)
        }
        Value::Object(properties) => (delete_object_property(properties, key), true),
        Value::Array(values)
            if array_descriptor_flag(&values, key, "configurable") == Some(false) =>
        {
            (Value::Array(values), false)
        }
        Value::Array(mut values) if values.is_arguments() => {
            Rc::make_mut(&mut values).delete_property(key);
            (Value::Array(values), true)
        }
        Value::Array(values) if key != "length" => (Value::Array(values), true),
        Value::Array(values) => (Value::Array(values), false),
        Value::Function(function) => delete_function_property(function, key),
        value => (value, true),
    }
}

fn delete_function_property(function: Rc<crate::value::FunctionValue>, key: &str) -> (Value, bool) {
    let configurable = descriptor_flag_in(&function.properties.borrow(), key, "configurable");
    if configurable == Some(false) {
        return (Value::Function(function), false);
    }
    let metadata = descriptor_key(key);
    function
        .properties
        .borrow_mut()
        .retain(|(name, _)| name != key && name != &metadata);
    (Value::Function(function), true)
}

fn delete_object_property(properties: Rc<crate::value::ObjectData>, key: &str) -> Value {
    let values = properties
            .iter()
            .filter(|(name, _)| name != key && name != &descriptor_key(key))
            .cloned()
            .collect();
    Value::Object(Rc::new(crate::value::ObjectData::with_private_slots(
        values,
        Rc::clone(&properties.private_slots),
    )))
}

fn define_property_value(target: Value, key: &str, value: Value) -> Value {
    match target {
        Value::Array(values) => set_array_property(values, key, value),
        target => set_property(target, key, value),
    }
}

fn define_array_descriptor(target: &mut Value, key: &str, descriptor: Vec<(String, Value)>) {
    let Value::Array(values) = target else { return };
    let mut values = Rc::clone(values);
    let accessor = descriptor
        .iter()
        .any(|(name, _)| matches!(name.as_str(), "get" | "set"));
    let writable = descriptor
        .iter()
        .rev()
        .find_map(|(name, value)| (name == "writable").then_some(value));
    if accessor || writable == Some(&Value::Boolean(false)) {
        if let Ok(index) = key.parse::<usize>() {
            Rc::make_mut(&mut values).disconnect_index(index);
        }
    }
    Rc::make_mut(&mut values).define_descriptor(
        key,
        Value::Object(Rc::new(crate::value::ObjectData::new(descriptor))),
    );
    *target = Value::Array(values);
}

fn array_descriptor_flag(values: &crate::value::ArrayData, key: &str, flag: &str) -> Option<bool> {
    let Value::Object(descriptor) = values.descriptor(key)? else {
        return None;
    };
    descriptor
        .iter()
        .rev()
        .find_map(|(name, value)| (name == flag).then_some(matches!(value, Value::Boolean(true))))
}

fn set_array_property(mut values: Rc<crate::value::ArrayData>, key: &str, value: Value) -> Value {
    if key == "length" {
        if values.is_arguments() {
            Rc::make_mut(&mut values).set_property(key, value);
            return Value::Array(values);
        }
        let length = value_to_number(&value).max(0.0) as usize;
        Rc::make_mut(&mut values).set_length(length);
        return Value::Array(values);
    }
    let Ok(index) = key.parse::<usize>() else {
        Rc::make_mut(&mut values).set_property(key, value);
        return Value::Array(values);
    };
    Rc::make_mut(&mut values).set_index(index, value);
    Value::Array(values)
}
