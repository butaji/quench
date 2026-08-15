fn error_builtin(
    builtin: Builtin,
    arguments: &[Value],
    receiver: Option<&Value>,
) -> Result<Value, VmError> {
    match builtin {
        Builtin::ThrowTypeError => restricted_arguments_error(receiver),
        Builtin::Error
        | Builtin::RangeError
        | Builtin::ReferenceError
        | Builtin::SyntaxError
        | Builtin::EvalError
        | Builtin::URIError
        | Builtin::AggregateError
        | Builtin::TypeError
        | Builtin::SuppressedError => {
            crate::construct::construct_value(&Value::Builtin(builtin), arguments)
        }
        Builtin::ErrorIsError => Ok(error_is_error(arguments.first())),
        Builtin::ErrorPrototypeToString => error_to_string(receiver),
        Builtin::ErrorPrototypeNameGetter => Ok(error_name_getter(receiver)?),
        Builtin::ErrorPrototypeMessageGetter => Ok(error_message_getter(receiver)?),
        Builtin::ErrorPrototypeCauseGetter => Ok(error_cause_getter(receiver)?),
        Builtin::ErrorPrototypeStackGetter => error_stack_getter(receiver),
        Builtin::ErrorPrototypeStackSetter => error_stack_setter(receiver, arguments),
        _ => Ok(Value::Undefined),
    }
}

fn restricted_arguments_error(receiver: Option<&Value>) -> Result<Value, VmError> {
    let Some(realm) = crate::vm::realm_id_for_intrinsic_receiver(receiver) else {
        return Err(crate::value::error::throw_type_error(
            "Restricted arguments property",
        ));
    };
    let error = crate::builtins::error(
        Builtin::TypeError,
        &[Value::String("Restricted arguments property".to_string())],
    );
    let error = if realm != crate::ops::RealmId::ROOT {
        let constructor =
            crate::vm::with_realm(realm, || crate::vm::realm_intrinsic(Builtin::TypeError))
                .unwrap_or(Value::Builtin(Builtin::TypeError));
        crate::builtins::set_property(error, "constructor", constructor)
    } else {
        error
    };
    Err(VmError::Thrown(error))
}

fn error_name_getter(receiver: Option<&Value>) -> Result<Value, VmError> {
    let value = error_receiver(receiver, "Error.prototype.name")?;
    crate::execute::get_property_result(value, "name")
}

fn error_message_getter(receiver: Option<&Value>) -> Result<Value, VmError> {
    let value = error_receiver(receiver, "Error.prototype.message")?;
    crate::execute::get_property_result(value, "message")
}

fn error_cause_getter(receiver: Option<&Value>) -> Result<Value, VmError> {
    let value = error_receiver(receiver, "Error.prototype.cause")?;
    crate::execute::get_property_result(value, "cause")
}

fn error_stack_getter(receiver: Option<&Value>) -> Result<Value, VmError> {
    let value = error_receiver(receiver, "Error.prototype.stack")?;
    if has_error_slot(value) {
        Ok(Value::String("Error".to_string()))
    } else {
        Ok(Value::Undefined)
    }
}

fn error_stack_setter(receiver: Option<&Value>, arguments: &[Value]) -> Result<Value, VmError> {
    let value = error_receiver(receiver, "Error.prototype.stack")?;
    let stack = arguments.first().ok_or_else(|| {
        crate::value::error::throw_type_error("Cannot set property 'stack' of error")
    })?;
    if let Some(home) = set_error_stack_home() {
        if crate::builtins::same_value(Some(&home), Some(value)) {
            return Err(crate::value::error::throw_type_error(
                "Cannot set property 'stack' of error",
            ));
        }
    }
    let Value::String(_) = stack else {
        return Err(crate::value::error::throw_type_error(
            "Stack value must be a string",
        ));
    };
    if matches!(value, Value::Proxy(_)) {
        define_proxy_stack(value, stack.clone())?;
        return Ok(Value::Undefined);
    }
    let key = Value::String("stack".to_string());
    if !matches!(
        crate::builtins::object::descriptor(Some(value), Some(&key))?,
        Value::Undefined
    ) {
        crate::builtins::set_property(value.clone(), "stack", stack.clone());
    } else {
        define_own_stack(value, stack.clone())?;
    }
    Ok(Value::Undefined)
}

fn define_own_stack(value: &Value, stack: Value) -> Result<(), VmError> {
    let descriptor = vec![
        ("value".to_string(), stack),
        ("writable".to_string(), Value::Boolean(true)),
        ("enumerable".to_string(), Value::Boolean(true)),
        ("configurable".to_string(), Value::Boolean(true)),
    ];
    let updated = crate::builtins::define_own_property(value, "stack", &descriptor)?;
    crate::locals::replace_value(value, &updated);
    Ok(())
}

fn define_proxy_stack(value: &Value, stack: Value) -> Result<Value, VmError> {
    let descriptor = Value::Object(Rc::new(crate::value::ObjectData::new(vec![
        ("value".to_string(), stack),
        ("writable".to_string(), Value::Boolean(true)),
        ("enumerable".to_string(), Value::Boolean(true)),
        ("configurable".to_string(), Value::Boolean(true)),
    ])));
    let result = crate::proxy::proxy_define_property(value, "stack", &descriptor)?;
    if matches!(result, Value::Boolean(false)) {
        return Err(crate::value::error::throw_type_error(
            "Proxy defineProperty trap returned false",
        ));
    }
    Ok(result)
}

fn set_error_stack_home() -> Option<Value> {
    let value = crate::execute::get_property(&crate::vm::current_global_object(), "Error");
    let Ok(value) = crate::execute::get_property_result(&value, "prototype") else {
        return None;
    };
    if !crate::value::is_object(&value) {
        return None;
    }
    Some(value)
}

fn has_error_slot(value: &Value) -> bool {
    match value {
        Value::Object(value) => value
            .iter()
            .any(|(key, _)| key == crate::builtins::ERROR_SLOT),
        Value::ObjectAlias(alias) => alias.0.borrow().upgrade().is_some_and(|value| {
            value
                .iter()
                .any(|(key, _)| key == crate::builtins::ERROR_SLOT)
        }),
        _ => false,
    }
}

fn error_to_string(receiver: Option<&Value>) -> Result<Value, VmError> {
    let value = error_receiver(receiver, "Error.prototype.toString")?;
    let name = match crate::execute::get_property_result(value, "name")? {
        Value::Undefined => "Error".to_string(),
        value => crate::conversion::to_string(&value)?,
    };
    let message = match crate::execute::get_property_result(value, "message")? {
        Value::Undefined => String::new(),
        value => crate::conversion::to_string(&value)?,
    };
    if name.is_empty() && message.is_empty() {
        Ok(Value::String(String::new()))
    } else if name.is_empty() {
        Ok(Value::String(message))
    } else if message.is_empty() {
        Ok(Value::String(name))
    } else {
        Ok(Value::String(format!("{name}: {message}")))
    }
}

fn error_is_error(value: Option<&Value>) -> Value {
    let Some(value) = value else {
        return Value::Boolean(false);
    };
    if !crate::value::is_object(value) {
        return Value::Boolean(false);
    }
    Value::Boolean(has_error_slot(value))
}

fn error_receiver<'a>(receiver: Option<&'a Value>, name: &str) -> Result<&'a Value, VmError> {
    let value = receiver.ok_or_else(|| {
        crate::value::error::throw_type_error(&format!("{name} called on non-object"))
    })?;
    if matches!(value, Value::Null | Value::Undefined) {
        return Err(crate::value::error::throw_type_error(&format!(
            "{name} called on non-object"
        )));
    }
    if !crate::value::is_object(value) {
        return Err(crate::value::error::throw_type_error(&format!(
            "{name} called on non-object"
        )));
    }
    Ok(value)
}
