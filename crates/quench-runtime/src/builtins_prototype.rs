pub(crate) fn prototype_value_of(receiver: Option<&Value>) -> Value {
    match receiver {
        None | Some(Value::Undefined) | Some(Value::Null) => Value::Null,
        Some(value) => value.clone(),
    }
}

pub(crate) fn function_prototype_to_string(receiver: Option<&Value>) -> Value {
    match receiver {
        Some(Value::Builtin(builtin)) => Value::String(format!("function {}() {{ [native code] }}", builtin_name(*builtin))),
        Some(Value::Function(_)) | Some(Value::BoundFunction(_)) => Value::String("function () {{ [native code] }}".to_string()),
        _ => Value::String(String::new()),
    }
}

pub(crate) fn function_prototype_value_of(receiver: Option<&Value>) -> Value { prototype_value_of(receiver) }
