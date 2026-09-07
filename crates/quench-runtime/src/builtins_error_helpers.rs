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
        Builtin::DOMException => (
            "DOMException",
            Builtin::DOMException,
            Builtin::DOMExceptionPrototype,
        ),
        _ => ("Error", Builtin::Error, Builtin::ErrorPrototype),
    }
}

pub(crate) fn dom_exception_value(
    arguments: &[Value],
) -> Result<Value, crate::execute::VmError> {
    let message = arguments
        .first()
        .filter(|value| !matches!(value, Value::Undefined))
        .map(crate::conversion::to_string)
        .transpose()?
        .unwrap_or_default();
    let (name, cause) = match arguments.get(1) {
        Some(Value::Object(options)) => {
            let options = Value::Object(options.clone());
            let name = crate::execute::get_property(&options, "name");
            let name = if matches!(name, Value::Undefined) {
                "Error".to_string()
            } else {
                crate::conversion::to_string(&name)?
            };
            let cause = if crate::with_scope::has_property(&options, "cause")? {
                Some(crate::execute::get_property(&options, "cause"))
            } else {
                None
            };
            (name, cause)
        }
        Some(value) if !matches!(value, Value::Undefined) => {
            (crate::conversion::to_string(value)?, None)
        }
        _ => ("Error".to_string(), None),
    };
    let code = match name.as_str() {
        "IndexSizeError" => 1.0,
        "DOMStringSizeError" => 2.0,
        "HierarchyRequestError" => 3.0,
        "WrongDocumentError" => 4.0,
        "InvalidCharacterError" => 5.0,
        "NoModificationAllowedError" => 7.0,
        "NotFoundError" => 8.0,
        "NotSupportedError" => 9.0,
        "InUseAttributeError" => 10.0,
        "InvalidStateError" => 11.0,
        "SyntaxError" => 12.0,
        "InvalidModificationError" => 13.0,
        "NamespaceError" => 14.0,
        "InvalidAccessError" => 15.0,
        "TypeMismatchError" => 17.0,
        "SecurityError" => 18.0,
        "NetworkError" => 19.0,
        "AbortError" => 20.0,
        "URLMismatchError" => 21.0,
        "QuotaExceededError" => 22.0,
        "TimeoutError" => 23.0,
        "InvalidNodeTypeError" => 24.0,
        "DataCloneError" => 25.0,
        _ => 0.0,
    };
    let stack = Value::String(format!("{name}: {message}"));
    let name_value = Value::String(name);
    let message_value = Value::String(message);
    let code_value = Value::Number(code);
    let prototype = crate::vm::realm_intrinsic(Builtin::DOMExceptionPrototype);
    let mut properties = vec![
        ("\0domexception_name".to_string(), name_value),
        ("\0domexception_message".to_string(), message_value),
        ("\0domexception_code".to_string(), code_value),
        ("\0prototype".to_string(), prototype),
        ("\0domexception".to_string(), Value::Boolean(true)),
        (crate::builtins::ERROR_SLOT.to_string(), Value::Boolean(true)),
        ("stack".to_string(), stack.clone()),
        (descriptor_key("stack"), non_enumerable_descriptor(&stack)),
    ];
    if let Some(cause) = cause {
        properties.push(("cause".to_string(), cause.clone()));
        properties.push((descriptor_key("cause"), non_enumerable_descriptor(&cause)));
    }
    Ok(Value::Object(Rc::new(ObjectData::new(properties))))
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

#[cfg(test)]
mod dom_exception_tests {
    use super::dom_exception_value;
    use crate::execute::get_property_result;
    use crate::value::Value;

    #[test]
    fn dom_exception_exposes_legacy_name_and_code() {
        let value = dom_exception_value(&[
            Value::String("cancelled".into()),
            Value::String("AbortError".into()),
        ])
        .expect("constructs");
        assert_eq!(
            get_property_result(&value, "name").expect("name"),
            Value::String("AbortError".into())
        );
        assert_eq!(
            get_property_result(&value, "message").expect("message"),
            Value::String("cancelled".into())
        );
        assert_eq!(
            get_property_result(&value, "code").expect("code"),
            Value::Number(20.0)
        );
    }
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
        // Compare the underlying strings directly without going through
        // to_string, which would throw on Symbol primitives. Two
        // distinct StringValues that both happen to wrap symbols must
        // compare by their inner String, not by ToString.
        let left_str = match left {
            Value::String(text) => Some(text.as_str()),
            Value::StringUnits(_) => None,
            _ => None,
        };
        let right_str = match right {
            Value::String(text) => Some(text.as_str()),
            Value::StringUnits(_) => None,
            _ => None,
        };
        if let (Some(l), Some(r)) = (left_str, right_str) {
            return l == r;
        }
        return crate::conversion::to_string(left)
            .ok()
            .zip(crate::conversion::to_string(right).ok())
            .is_some_and(|(left, right)| left == right);
    }
    same_value_objects(left, right)
}

pub(crate) fn set_property(target: Value, key: &str, value: Value) -> Value {
    if crate::builtins::descriptor_flag(&target, key, "writable") == Some(false) {
        return target;
    }
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
        Value::Object(properties)
            if properties.iter().any(|(name, value)| {
                (name == "\0quench:async_hooks:mutable" || name == "\0quench:host:mutable")
                    && matches!(value, Value::Boolean(true))
            }) =>
        {
            // Async-hook resources are host-owned identity objects. Their
            // init callback is allowed to attach arbitrary user properties;
            // publish those writes in place so the host's canonical resource
            // observes the same object the VM callback received.
            unsafe {
                (&mut *(Rc::as_ptr(&properties) as *mut ObjectData))
                    .set_property_in_place(key, value);
            }
            Value::Object(properties)
        }
        Value::Object(properties) if boxed_string_immutable_key(&properties, key) => {
            Value::Object(properties)
        }
        Value::Object(properties)
            if descriptor_flag_in(properties.as_ref(), key, "writable") == Some(false) =>
        {
            Value::Object(properties)
        }
        Value::Object(properties) => {
            builtins_cells::set_object_property(properties, key, value)
        }
        Value::ObjectAlias(alias) => set_object_alias_property(alias, key, value),
        Value::Array(values) if array_descriptor_flag(&values, key, "writable") == Some(false) => {
            Value::Array(values)
        }
        Value::Array(values) => set_array_property(values, key, value),
        Value::Function(function) => set_function_property(function, key, value),
        Value::Builtin(builtin) => {
            // Intrinsic objects are represented as stable builtin identities;
            // ordinary assignment records the value in the shared override
            // table so all existing references observe the same mutation.
            let descriptor = Value::Object(Rc::new(ObjectData::new(vec![
                ("value".to_string(), value),
                ("writable".to_string(), Value::Boolean(true)),
                ("enumerable".to_string(), Value::Boolean(false)),
                ("configurable".to_string(), Value::Boolean(true)),
            ])));
            crate::builtins::write_intrinsic_override(builtin, key, descriptor);
            Value::Builtin(builtin)
        }
        _ => set_property_tail(target, key, value),
    }
}
