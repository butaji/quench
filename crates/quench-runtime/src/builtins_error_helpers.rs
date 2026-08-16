fn error_parts(builtin: Builtin) -> (&'static str, Builtin, Builtin) {
    match builtin {
        Builtin::RangeError => (
            "RangeError",
            Builtin::RangeError,
            Builtin::RangeErrorPrototype,
        ),
        Builtin::ReferenceError => (
            "ReferenceError",
            Builtin::ReferenceError,
            Builtin::ReferenceErrorPrototype,
        ),
        Builtin::SyntaxError => (
            "SyntaxError",
            Builtin::SyntaxError,
            Builtin::SyntaxErrorPrototype,
        ),
        Builtin::EvalError => ("EvalError", Builtin::EvalError, Builtin::EvalErrorPrototype),
        Builtin::URIError => ("URIError", Builtin::URIError, Builtin::URIErrorPrototype),
        Builtin::AggregateError => (
            "AggregateError",
            Builtin::AggregateError,
            Builtin::ErrorPrototype,
        ),
        Builtin::TypeError => ("TypeError", Builtin::TypeError, Builtin::TypeErrorPrototype),
        Builtin::SuppressedError => (
            "SuppressedError",
            Builtin::SuppressedError,
            Builtin::ErrorPrototype,
        ),
        Builtin::Error => ("Error", Builtin::Error, Builtin::ErrorPrototype),
        _ => ("Error", Builtin::Error, Builtin::ErrorPrototype),
    }
}

pub(crate) fn suppressed_error(arguments: &[Value]) -> Result<Value, crate::execute::VmError> {
    let error = arguments.first().cloned().unwrap_or(Value::Undefined);
    let suppressed = arguments.get(1).cloned().unwrap_or(Value::Undefined);
    let message = arguments
        .get(2)
        .filter(|value| !matches!(value, Value::Undefined))
        .map(crate::conversion::to_string)
        .transpose()?;
    let mut properties = vec![
        (
            "name".to_string(),
            Value::String("SuppressedError".to_string()),
        ),
        (
            "\0prototype".to_string(),
            Value::Builtin(Builtin::SuppressedErrorPrototype),
        ),
    ];
    let mut data_properties = Vec::new();
    if let Some(message) = message {
        data_properties.push(("message".to_string(), Value::String(message)));
    }
    data_properties.push(("error".to_string(), error));
    data_properties.push(("suppressed".to_string(), suppressed));
    for (key, value) in data_properties {
        properties.push((descriptor_key(&key), non_enumerable_descriptor(&value)));
        properties.push((key, value));
    }
    properties.push((
        "constructor".to_string(),
        Value::Builtin(Builtin::SuppressedError),
    ));
    properties.push((
        crate::builtins::ERROR_SLOT.to_string(),
        Value::Boolean(true),
    ));
    Ok(Value::Object(Rc::new(ObjectData::new(properties))))
}

include!("builtins_descriptor_core.rs");
pub(crate) fn same_value(left: Option<&Value>, right: Option<&Value>) -> bool {
    let (Some(left), Some(right)) = (left, right) else {
        return matches!((left, right), (None, None));
    };
    if let (Value::Number(left), Value::Number(right)) = (left, right) {
        return (left.is_nan() && right.is_nan())
            || (left == right && left.is_sign_negative() == right.is_sign_negative());
    }
    if matches!(left, Value::String(_) | Value::StringUnits(_))
        && matches!(right, Value::String(_) | Value::StringUnits(_))
    {
        return crate::conversion::to_string(left)
            .ok()
            .zip(crate::conversion::to_string(right).ok())
            .is_some_and(|(left, right)| left == right);
    }
    same_value_objects(left, right)
}

pub(crate) fn set_property(target: Value, key: &str, value: Value) -> Value {
    if let Some(result) = crate::typed_array_prototype::set(&target, key, value.clone()) {
        return result;
    }
    if let Some(result) = crate::typed_array_ops::set_property(&target, key, &value) {
        return result.unwrap_or(target);
    }
    if let Some(result) = set_prototype_slot(&target, key, value.clone()) {
        return result;
    }
    if let Some(result) = set_promise_property(&target, key, value.clone()) {
        return result;
    }
    match target {
        Value::Object(properties) if boxed_string_immutable_key(&properties, key) => {
            Value::Object(properties)
        }
        Value::Object(properties)
            if descriptor_flag_in(&properties, key, "writable") == Some(false) =>
        {
            Value::Object(properties)
        }
        Value::Object(properties) => builtins_cells::set_object_property(properties, key, value),
        Value::ObjectAlias(alias) => set_object_alias_property(alias, key, value),
        Value::Array(values) if array_descriptor_flag(&values, key, "writable") == Some(false) => {
            Value::Array(values)
        }
        Value::Array(values) => set_array_property(values, key, value),
        Value::Function(function) => set_function_property(function, key, value),
        _ => set_property_tail(target, key, value),
    }
}


