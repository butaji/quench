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
        Builtin::CallSiteGetFileName => call_site_field(receiver, "__file", false),
        Builtin::CallSiteGetLineNumber => call_site_field(receiver, "__line", true),
        Builtin::CallSiteGetColumnNumber => call_site_field(receiver, "__column", true),
        Builtin::CallSiteGetFunctionName => call_site_field(receiver, "__name", false),
        Builtin::CallSiteIsEval => Ok(crate::value::Value::Boolean(false)),
        Builtin::CallSiteGetEvalOrigin => Ok(crate::value::Value::Null),
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
    let Value::String(text) = stack else {
        return Err(crate::value::error::throw_type_error("Stack value must be a string"));
    };
    if crate::conversion::is_symbol_string(text) {
        return Err(crate::value::error::throw_type_error("Stack value must be a string"));
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
        let updated = crate::properties::set_with_receiver(value, "stack", stack, value)?;
        if !updated {
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

/// `Error.captureStackTrace(target)` — attach a `stack` property to `target`.
///
/// The host does not retain call-frame history, so it attaches an empty
/// stack string. This preserves the observable contract real-world modules
/// depend on (the function exists, is callable, and `target.stack` is a
/// string) without fabricating call-site data.
fn capture_stack_trace(arguments: &[Value]) -> Result<Value, VmError> {
    let Some(target) = arguments.first() else {
        return Err(crate::value::error::throw_type_error(
            "The 'target' argument must be an object",
        ));
    };
    if matches!(
        target,
        crate::value::Value::Undefined | crate::value::Value::Null
    ) {
        return Err(crate::value::error::throw_type_error(
            "The 'target' argument must be an object",
        ));
    }
    let frames = crate::frame_stack::snapshot();
    let sites: Vec<Value> = frames.iter().map(build_call_site).collect();
    let prepared = crate::execute::get_property(
        &crate::value::Value::Builtin(crate::ops::Builtin::Error),
        "prepareStackTrace",
    );
    let stack = if crate::conversion::is_callable(&prepared) {
        let frame_list = crate::host_api::array(sites);
        crate::vm::call_value(&prepared, &crate::value::Value::Undefined, &[
            target.clone(),
            frame_list,
        ])?
    } else {
        let mut text = String::from("Error\n");
        for frame in &frames {
            text.push_str("\n    at ");
            if !frame.function.is_empty() {
                text.push_str(&frame.function);
                text.push_str(" (");
                text.push_str(&frame.filename);
                text.push(')');
            } else {
                text.push_str(&frame.filename);
            }
        }
        crate::value::Value::String(text)
    };
    let descriptor = crate::host_api::object(vec![
        ("value".to_string(), stack),
        ("writable".to_string(), crate::value::Value::Boolean(true)),
        ("enumerable".to_string(), crate::value::Value::Boolean(false)),
        ("configurable".to_string(), crate::value::Value::Boolean(true)),
    ]);
    let updated = crate::execute::define_property(target.clone(), "stack", descriptor)?;
    crate::execute::replace_value(target, &updated);
    Ok(crate::value::Value::Undefined)
}

/// Build a `CallSite`-shaped object exposing the V8 `CallSite` read methods
/// as callable builtins. The object carries its one real frame's data in
/// hidden `__*` properties the methods read back.
fn build_call_site(frame: &crate::frame_stack::FrameInfo) -> Value {
    crate::host_api::object(vec![
        (
            "__file".to_string(),
            crate::value::Value::String(frame.filename.clone()),
        ),
        (
            "__name".to_string(),
            crate::value::Value::String(frame.function.clone()),
        ),
        ("__line".to_string(), crate::value::Value::Number(0.0)),
        ("__column".to_string(), crate::value::Value::Number(0.0)),
        (
            "getFileName".to_string(),
            crate::value::Value::Builtin(crate::ops::Builtin::CallSiteGetFileName),
        ),
        (
            "getLineNumber".to_string(),
            crate::value::Value::Builtin(crate::ops::Builtin::CallSiteGetLineNumber),
        ),
        (
            "getColumnNumber".to_string(),
            crate::value::Value::Builtin(crate::ops::Builtin::CallSiteGetColumnNumber),
        ),
        (
            "getFunctionName".to_string(),
            crate::value::Value::Builtin(crate::ops::Builtin::CallSiteGetFunctionName),
        ),
        (
            "isEval".to_string(),
            crate::value::Value::Builtin(crate::ops::Builtin::CallSiteIsEval),
        ),
        (
            "getEvalOrigin".to_string(),
            crate::value::Value::Builtin(crate::ops::Builtin::CallSiteGetEvalOrigin),
        ),
    ])
}

/// Read a CallSite field off the receiver (`this`). String fields return null
/// when absent/empty (matching V8); numeric fields return their value.
fn call_site_field(
    receiver: Option<&Value>,
    key: &str,
    numeric: bool,
) -> Result<Value, VmError> {
    let Some(site) = receiver else {
        return Ok(crate::value::Value::Null);
    };
    let value = crate::execute::get_property(site, key);
    if numeric {
        return Ok(value);
    }
    match value {
        crate::value::Value::String(text) if !text.is_empty() => Ok(crate::value::Value::String(text)),
        _ => Ok(crate::value::Value::Null),
    }
}
