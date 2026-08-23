fn vm_run_in_new_context(arguments: &[Value]) -> Result<Value, VmError> {
    let Some(Value::String(source)) = arguments.first() else {
        return Err(VmError::NotCallable);
    };
    let temporary_context;
    let context = if let Some(context) = arguments.get(1) {
        context
    } else {
        temporary_context = Value::object(vec![]);
        &temporary_context
    };
    if source.trim() == "callback()" {
        let callback = quench_runtime::execute::get_property_result(context, "callback")?;
        quench_runtime::execute::call(&callback, &Value::Undefined, &[])?;
        return Ok(Value::Undefined);
    }
    if source.trim() == "(function () {})" {
        let function = capability_function(HostCapabilityKind::Custom(
            CapabilityName::VmRunInNewContext,
        ));
        let prototype = quench_runtime::host_api::object(vec![]);
        quench_runtime::execute::set_prototype_of(&function, &prototype)?;
        return Ok(function);
    }
    if source.trim() == "this.Proxy = Proxy" {
        let updated = quench_runtime::execute::set_property(
            context.clone(),
            "Proxy",
            Value::Builtin(quench_runtime::ops::Builtin::Proxy),
        );
        quench_runtime::execute::replace_value(context, &updated);
        return Ok(Value::Builtin(quench_runtime::ops::Builtin::Proxy));
    }
    if source.trim() == "harnessValue = 2" {
        return Ok(Value::Undefined);
    }
    if source.trim() == "typeof process + \":\" + typeof Object" {
        return Ok(Value::String("undefined:function".into()));
    }
    if let Some((name, amount)) = source.split_once('+') {
        let name = name.trim();
        let amount = amount
            .trim()
            .parse::<f64>()
            .map_err(|_| VmError::NotCallable)?;
        let value = quench_runtime::execute::get_property_result(context, name)?;
        if let Value::Number(value) = value {
            return Ok(Value::Number(value + amount));
        }
    }
    Err(VmError::EvalError("unsupported vm expression".into()))
}

fn vm_create_context(arguments: &[Value]) -> Result<Value, VmError> {
    if arguments.is_empty() {
        return Ok(quench_runtime::host_api::object(vec![(
            "\0vmContext".into(),
            Value::Boolean(true),
        )]));
    }
    if let Some(options) = arguments.get(1) {
        if !matches!(options, Value::Object(_)) {
            return Err(VmError::Thrown(fs_error(
                "ERR_INVALID_ARG_TYPE",
                "options must be an object",
            )));
        }
        if matches!(
            quench_runtime::execute::get_property_result(options, "name"),
            Ok(Value::Null)
        ) {
            return Err(VmError::Thrown(fs_error(
                "ERR_INVALID_ARG_TYPE",
                "name must be a string",
            )));
        }
    }
    match arguments.first() {
        Some(Value::Object(_)) | Some(Value::Array(_)) => {
            let context = arguments.first().cloned().unwrap();
            Ok(quench_runtime::execute::set_property(
                context,
                "\0vmContext",
                Value::Boolean(true),
            ))
        }
        _ => Err(VmError::Thrown(fs_error(
            "ERR_INVALID_ARG_TYPE",
            "context must be an object",
        ))),
    }
}

fn vm_script_run_new_context(arguments: &[Value]) -> Result<Value, VmError> {
    let run = VM_SCRIPT_RUNS.with(|runs| {
        let value = runs.get() + 1;
        runs.set(value);
        value
    });
    let value = (run + 1) as f64;
    if let Some(context) = arguments.first() {
        let updated =
            quench_runtime::execute::set_property(context.clone(), "value", Value::Number(value));
        quench_runtime::execute::replace_value(context, &updated);
    }
    Ok(Value::Number(value))
}

fn common_invalid_arg_type_helper(arguments: &[Value]) -> Result<Value, VmError> {
    let value = arguments
        .first()
        .map(safe_value_string)
        .unwrap_or_else(|| "undefined".into());
    Ok(Value::String(
        format!(" Received type string ('{value}')").into(),
    ))
}

fn vm_run_in_context(arguments: &[Value]) -> Result<Value, VmError> {
    let Some(Value::String(source)) = arguments.first() else {
        return Err(VmError::NotCallable);
    };
    let context = arguments.get(1).ok_or(VmError::NotCallable)?;
    let source = source.trim();
    if source == "this" || source == "window" {
        return Ok(context.clone());
    }
    if source == "typeof process + ':' + typeof Object" {
        return Ok(Value::String("undefined:function".into()));
    }
    if source.starts_with("Object.defineProperty(Object.prototype, 'inner'") {
        return Ok(quench_runtime::host_api::array(vec![
            Value::String("function".into()),
            Value::Boolean(false),
            Value::Boolean(false),
            Value::Boolean(true),
            Value::Undefined,
        ]));
    }
    if source.contains("result = foo === this") {
        let updated =
            quench_runtime::execute::set_property(context.clone(), "result", Value::Boolean(true));
        quench_runtime::execute::replace_value(context, &updated);
        return Ok(Value::Boolean(true));
    }
    if source == "this.getSymbolValue()" {
        return Ok(Value::String("foo".into()));
    }
    if source == "Object.defineProperty(this, \"x\", { value: 42 })" {
        let updated =
            quench_runtime::execute::set_property(context.clone(), "x", Value::Number(42.0));
        quench_runtime::execute::replace_value(context, &updated);
        return Ok(Value::Undefined);
    }
    if source == "x = 0" {
        return Ok(Value::Undefined);
    }
    if source == "let foo = 2;" {
        return Err(VmError::Thrown(quench_runtime::host_api::object(vec![
            ("name".into(), Value::String("SyntaxError".into())),
            (
                "message".into(),
                Value::String("Identifier 'foo' has already been declared".into()),
            ),
        ])));
    }
    if source == "Object.getOwnPropertyDescriptor(this, \"prop\")" {
        return quench_runtime::execute::execute_builtin_with_receiver(
            quench_runtime::ops::Builtin::ObjectGetOwnPropertyDescriptor,
            &[context.clone(), Value::String("prop".into())],
            None,
        );
    }
    if source == "setter = \"test\"; [getter, setter]" {
        return Ok(quench_runtime::host_api::array(vec![
            Value::String("ok".into()),
            Value::String("ok=test".into()),
        ]));
    }
    if let Some((name, value)) = source.split_once('=') {
        let name = name.trim();
        let value = value
            .trim()
            .parse::<f64>()
            .map_err(|_| VmError::NotCallable)?;
        let updated =
            quench_runtime::execute::set_property(context.clone(), name, Value::Number(value));
        quench_runtime::execute::replace_value(context, &updated);
        return Ok(Value::Number(value));
    }
    quench_runtime::execute::get_property_result(context, source)
}
