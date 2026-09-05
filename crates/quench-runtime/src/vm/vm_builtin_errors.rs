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
        Builtin::ErrorCaptureStackTrace => capture_stack_trace(arguments),
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
    // Error instances materialize `stack` as a configurable own data
    // property. Once user code deletes it, the prototype accessor must not
    // resurrect a synthetic stack from the internal error marker.
    if !matches!(
        crate::builtins::object::has_own_property(
            Some(value),
            Some(&Value::String("stack".to_string())),
        ),
        Value::Boolean(true)
    ) {
        return Ok(Value::Undefined);
    }
    let error_constructor = crate::execute::get_property(
        &crate::vm::current_global_object(),
        "Error",
    );
    let prepare = crate::execute::get_property_result(&error_constructor, "prepareStackTrace")?;
    if crate::conversion::is_callable(&prepare) {
        return crate::execute::call(
            &prepare,
            &Value::Undefined,
            &[value.clone(), Value::array(Vec::new())],
        );
    }
    if has_error_slot(value) {
        let mut stack = stack_text(value)?;
        // Error construction can happen outside an active function frame
        // (for example, before an enumerable `stack` descriptor is copied).
        // V8 still materializes the current script location lazily. Preserve
        // the configured zero-frame limit, otherwise derive one host frame
        // from the canonical source context.
        let limit = crate::execute::get_property(
            &crate::vm::current_global_object(),
            "Error",
        );
        let limit = crate::execute::get_property(&limit, "stackTraceLimit");
        let has_frames = stack.contains('\n');
        if !has_frames
            && !matches!(limit, Value::Number(value) if value <= 0.0)
            && crate::vm::current_context().source_name().is_some()
        {
            let frames = crate::vm::vm_ops::call_stack_frames();
            let frame = frames
                .last()
                .cloned()
                .unwrap_or_else(|| "<anonymous>".to_string());
            let filename = crate::vm::current_context()
                .source_name()
                .map(|name| name.to_string())
                .unwrap_or_default();
            stack.push_str(&format!("\n    at {frame} ({filename}:1:1)"));
        }
        Ok(Value::String(stack))
    } else {
        Ok(Value::Undefined)
    }
}

fn capture_stack_trace(arguments: &[Value]) -> Result<Value, VmError> {
    let target = arguments.first().ok_or_else(|| {
        crate::value::error::throw_type_error("Error.captureStackTrace requires an object")
    })?;
    if !crate::value::is_object(target) {
        return Err(crate::value::error::throw_type_error(
            "Error.captureStackTrace requires an object",
        ));
    }
    let stack = Value::String(stack_text(target)?);
    let descriptor = Value::Object(std::rc::Rc::new(crate::value::ObjectData::new(vec![
        ("value".to_string(), stack),
        ("writable".to_string(), Value::Boolean(true)),
        ("enumerable".to_string(), Value::Boolean(false)),
        ("configurable".to_string(), Value::Boolean(true)),
    ])));
    let updated = crate::builtins::define_property(&[
        target.clone(),
        Value::String("stack".to_string()),
        descriptor,
    ])?;
    crate::locals::replace_value(target, &updated);
    Ok(Value::Undefined)
}

fn stack_text(value: &Value) -> Result<String, VmError> {
    let name = crate::conversion::to_string(&crate::execute::get_property_result(value, "name")?)?;
    let message =
        crate::conversion::to_string(&crate::execute::get_property_result(value, "message")?)?;
    Ok(if message.is_empty() {
        name
    } else if name.is_empty() {
        message
    } else {
        format!("{name}: {message}")
    })
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
    if error_stack_setter_dispatch(value, stack)? {
        return Ok(Value::Undefined);
    }
    Err(crate::value::error::throw_type_error("Cannot set property 'stack' of error"))
}

fn error_stack_setter_dispatch(value: &Value, stack: &Value) -> Result<bool, VmError> {
    let owns_stack = if matches!(value, Value::Proxy(_)) {
        crate::proxy::proxy_get_own_property_descriptor(value, "stack")? != Value::Undefined
    } else {
        matches!(
            crate::builtins::object::has_own_property(Some(value), Some(&Value::String("stack".to_string()))),
            Value::Boolean(true)
        )
    };
    if owns_stack {
        let updated = if matches!(value, Value::Proxy(_)) {
            crate::proxy::proxy_set(value, "stack", stack, Some(value))?
        } else {
            let updated = crate::execute::set_property_in_place(
                value,
                "stack",
                stack.clone(),
            );
            if updated {
                let _ = crate::execute::set_property_in_place(
                    value,
                    "\0quench:stack_decorated",
                    Value::Boolean(true),
                );
            }
            Value::Boolean(updated)
        };
        if !crate::execute::is_truthy(&updated) {
            return Ok(false);
        }
    } else if matches!(value, Value::Proxy(_)) {
        define_proxy_stack(value, stack.clone())?;
    } else {
        define_own_stack(value, stack.clone())?;
    }
    Ok(true)
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
        Value::Object(object) => object.iter().any(|(key, _)| {
            key == crate::builtins::ERROR_SLOT || key == "\0domexception"
        }),
        Value::ObjectAlias(alias) => alias.0.borrow().upgrade().is_some_and(|object| {
            object.iter().any(|(key, _)| {
                key == crate::builtins::ERROR_SLOT || key == "\0domexception"
            })
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
    let node_coded = matches!(
        crate::execute::get_property_result(value, "\0node_error_to_string_code")?,
        Value::Boolean(true)
    );
    if name == "SystemError" || node_coded {
        if let Value::String(code) = crate::execute::get_property_result(value, "code")? {
            if !code.is_empty() {
                return Ok(Value::String(if message.is_empty() {
                    format!("{name} [{code}]")
                } else {
                    format!("{name} [{code}]: {message}")
                }));
            }
        }
    }
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
