fn define_array_descriptor(target: &mut Value, key: &str, descriptor: Vec<(String, Value)>) {
    let Value::Array(values) = target else { return };
    let mut values = Rc::clone(values);
    let writable = descriptor
        .iter()
        .rev()
        .find_map(|(name, value)| (name == "writable").then_some(value));
    if writable == Some(&Value::Boolean(false)) {
        if let Ok(index) = key.parse::<usize>() {
            Rc::make_mut(&mut values).disconnect_index(index);
        }
    }
    Rc::make_mut(&mut values).define_descriptor(key, Value::Object(Rc::new(descriptor)));
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
