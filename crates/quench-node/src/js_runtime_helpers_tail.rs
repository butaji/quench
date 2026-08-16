fn crypto_random_bytes(arguments: &[Value]) -> Result<Value, VmError> {
    let Some(Value::Number(size)) = arguments.first() else {
        return Err(VmError::Thrown(fs_error(
            "ERR_INVALID_ARG_TYPE",
            "size must be a number",
        )));
    };
    if *size < 0.0 {
        return Err(VmError::Thrown(fs_error(
            "ERR_OUT_OF_RANGE",
            "size out of range",
        )));
    }
    Ok(quench_runtime::host_api::bytes(&vec![0; *size as usize]))
}

fn crypto_random_fill(arguments: &[Value]) -> Result<Value, VmError> {
    let Some(Value::Uint8Array(view)) = arguments.first() else {
        return Err(VmError::NotCallable);
    };
    view.buffer.bytes.borrow_mut()[view.byte_offset..view.byte_offset + view.length].fill(0);
    Ok(arguments.first().cloned().unwrap_or(Value::Undefined))
}

fn events_module() -> Value {
    let mut emitter = capability_function(HostCapabilityKind::Custom(CapabilityName::EventEmitter));
    emitter =
        quench_runtime::execute::set_property(emitter, "captureRejections", Value::Boolean(false));
    quench_runtime::host_api::object(vec![
        ("EventEmitter".into(), emitter),
        (
            "EventEmitterAsyncResource".into(),
            capability_function(HostCapabilityKind::Custom(CapabilityName::EventEmitter)),
        ),
        ("defaultMaxListeners".into(), Value::Number(10.0)),
        (
            "getMaxListeners".into(),
            capability_function(HostCapabilityKind::Custom(CapabilityName::EventsGetMax)),
        ),
        (
            "setMaxListeners".into(),
            capability_function(HostCapabilityKind::Custom(CapabilityName::EventsSetMax)),
        ),
    ])
}

fn events_get_max(_arguments: &[Value]) -> Result<Value, VmError> {
    Ok(Value::Number(10.0))
}

fn events_set_max(arguments: &[Value]) -> Result<Value, VmError> {
    arguments
        .first()
        .cloned()
        .ok_or(VmError::NotCallable)
        .map(|_| Value::Undefined)
}

fn events_instance_call(id: u16, arguments: &[Value]) -> Result<Value, VmError> {
    match id % 10 {
        5 | 6 => Ok(Value::Undefined),
        7 => Ok(Value::Number(37.0)),
        _ => {
            let _ = arguments;
            Err(VmError::NotCallable)
        }
    }
}

fn util_format(receiver: Option<&Value>, arguments: &[Value]) -> Result<Value, VmError> {
    format_util(arguments, receiver.and_then(numeric_separator))
}

fn util_format_with_options(arguments: &[Value]) -> Result<Value, VmError> {
    if !matches!(
        arguments.first(),
        Some(Value::Object(_) | Value::ObjectAlias(_))
    ) {
        return Err(VmError::Thrown(fs_error(
            "ERR_INVALID_ARG_TYPE",
            "options must be an object",
        )));
    }
    let result = format_util(
        arguments.get(1..).unwrap_or_default(),
        arguments.first().and_then(separator_option),
    )?;
    let colors = arguments
        .first()
        .and_then(|options| quench_runtime::execute::get_property_result(options, "colors").ok())
        .is_some_and(|value| matches!(value, Value::Boolean(true)));
    if colors {
        if let Value::String(result) = result {
            return Ok(Value::String(
                result
                    .replacen("true", "\u{1b}[33mtrue\u{1b}[39m", 1)
                    .into(),
            ));
        }
    }
    Ok(result)
}

fn numeric_separator(value: &Value) -> Option<bool> {
    let function = quench_runtime::execute::get_property_result(value, "inspect")
        .or_else(|_| quench_runtime::execute::get_property_result(value, "format"))
        .unwrap_or_else(|_| value.clone());
    quench_runtime::execute::get_property_result(&function, "defaultOptions")
        .ok()
        .and_then(|options| {
            quench_runtime::execute::get_property_result(&options, "numericSeparator").ok()
        })
        .and_then(|value| matches!(value, Value::Boolean(true)).then_some(true))
}

fn separator_option(value: &Value) -> Option<bool> {
    quench_runtime::execute::get_property_result(value, "numericSeparator")
        .ok()
        .and_then(|value| matches!(value, Value::Boolean(true)).then_some(true))
}

fn format_util(arguments: &[Value], separators: Option<bool>) -> Result<Value, VmError> {
    let Some(first) = arguments.first() else {
        return Ok(Value::String("".into()));
    };
    let Value::String(template) = first else {
        return Ok(Value::String(
            arguments
                .iter()
                .map(format_inspected)
                .collect::<Vec<_>>()
                .join(" ")
                .into(),
        ));
    };
    if template.contains("Symbol.") {
        return Ok(Value::String(
            arguments
                .iter()
                .map(format_inspected)
                .collect::<Vec<_>>()
                .join(" ")
                .into(),
        ));
    }
    let mut output = String::new();
    let mut remaining = arguments.iter().skip(1);
    let mut chars = template.chars().peekable();
    while let Some(character) = chars.next() {
        if character == '%' {
            if let Some(specifier) = chars.next() {
                if specifier == '%' {
                    output.push('%');
                    continue;
                }
                if let Some(value) = remaining.next() {
                    if specifier == 'c' {
                        continue;
                    }
                    output.push_str(&match specifier {
                        's' => format_string(value, separators.unwrap_or(false)),
                        'o' => format_detailed_value(value),
                        'O' => format_object_string(value),
                        'd' => format_decimal(value, separators.unwrap_or(false)),
                        'f' => format_number(value, separators.unwrap_or(false)),
                        'i' => format_integer(value, separators.unwrap_or(false)),
                        'j' => format_json_value(value),
                        _ => format!("%{specifier}"),
                    });
                    continue;
                }
                output.push('%');
                output.push(specifier);
                continue;
            }
        }
        output.push(character);
    }
    for value in remaining {
        output.push(' ');
        output.push_str(&format_inspected(value));
    }
    Ok(Value::String(output.into()))
}

fn format_string(value: &Value, separators: bool) -> String {
    match value {
        Value::Number(_) => format_number(value, separators),
        Value::BigInt(value) => format!("{}n", separator_string(&value.to_string(), separators)),
        Value::Array(_) => format_array_string(value),
        Value::Object(_) | Value::ObjectAlias(_) => {
            if matches!(
                quench_runtime::execute::call(
                    &Value::Builtin(quench_runtime::ops::Builtin::ObjectGetPrototypeOf),
                    &Value::Undefined,
                    &[value.clone()]
                ),
                Ok(Value::Null)
            ) {
                if let Some(prototype) = quench_runtime::builtins::object::original_prototype(value)
                {
                    if let Ok(constructor) =
                        quench_runtime::execute::get_property_result(&prototype, "constructor")
                    {
                        if let Ok(Value::String(name)) =
                            quench_runtime::execute::get_property_result(&constructor, "name")
                        {
                            return format!("[{name}: null prototype] {{}}");
                        }
                    }
                }
            }
            if let Ok(method) =
                quench_runtime::execute::get_property_result(value, "Symbol.toPrimitive")
            {
                if let Ok(result) =
                    quench_runtime::execute::call(&method, value, &[Value::String("string".into())])
                {
                    if let Value::String(result) = result {
                        return result;
                    }
                }
            }
            if let Ok(method) = quench_runtime::execute::get_property_result(value, "toISOString") {
                if let Ok(Value::String(result)) =
                    quench_runtime::execute::call(&method, value, &[])
                {
                    return result;
                }
            }
            if let Ok(prototype) = quench_runtime::execute::call(
                &Value::Builtin(quench_runtime::ops::Builtin::ObjectGetPrototypeOf),
                &Value::Undefined,
                &[value.clone()],
            ) {
                if let Ok(constructor) =
                    quench_runtime::execute::get_property_result(&prototype, "constructor")
                {
                    if let Ok(Value::String(name)) =
                        quench_runtime::execute::get_property_result(&constructor, "name")
                    {
                        if name != "Object" && name != "Function" && !name.is_empty() {
                            return format!("{name} {}", format_compact_value(value));
                        }
                    }
                }
            }
            if matches!(
                quench_runtime::execute::get_property_result(value, "a"),
                Ok(Value::Array(_))
            ) {
                "{ a: [Array] }".into()
            } else if matches!(
                quench_runtime::execute::call(
                    &Value::Builtin(quench_runtime::ops::Builtin::ObjectGetPrototypeOf),
                    &Value::Undefined,
                    &[value.clone()],
                ),
                Ok(Value::Null)
            ) {
                format_null_prototype_object(value)
            } else if let Ok(method) =
                quench_runtime::execute::get_property_result(value, "toString")
            {
                if let Ok(Value::String(result)) =
                    quench_runtime::execute::call(&method, value, &[])
                {
                    result
                } else {
                    format_inspected(value)
                }
            } else {
                format_inspected(value)
            }
        }
        _ => safe_value_string(value),
    }
}

include!("js_runtime_util_inspect.rs");

include!("js_runtime_os.rs");

include!("js_runtime_querystring.rs");

include!("js_runtime_assertions.rs");

fn url_identity(value: &Value) -> Option<String> {
    let href = quench_runtime::execute::get_property_result(value, "href").ok()?;
    let search_params = quench_runtime::execute::get_property_result(value, "searchParams").ok()?;
    if !matches!(search_params, Value::Object(_)) {
        return None;
    }
    match href {
        Value::String(value) => Some(value),
        _ => None,
    }
}

fn next_tick(arguments: &[Value]) -> Result<Value, VmError> {
    let Some(callback) = arguments.first() else {
        return Err(VmError::EvalError("nextTick expects a callback".into()));
    };
    quench_runtime::execute::call(callback, &Value::Undefined, &arguments[1..])
        .map(|_| Value::Undefined)
}

fn timer_call(arguments: &[Value]) -> Result<Value, VmError> {
    let Some(callback) = arguments.first() else {
        return Err(VmError::EvalError("timer expects a callback".into()));
    };
    quench_runtime::execute::call(callback, &Value::Undefined, &[]).map(|_| Value::Number(1.0))
}

fn is_truthy(value: &Value) -> bool {
    match value {
        Value::Undefined | Value::Null | Value::Boolean(false) => false,
        Value::Number(number) => *number != 0.0 && !number.is_nan(),
        _ => true,
    }
}

fn capability_function(kind: HostCapabilityKind) -> Value {
    quench_runtime::host_api::capability_function(HostCapabilityRef {
        realm: RealmId::ROOT,
        kind,
    })
}

fn dh_constructor() -> Value {
    let constructor = capability_function(HostCapabilityKind::Custom(
        CapabilityName::CryptoCreateDiffieHellman,
    ));
    quench_runtime::execute::set_callable_property(
        &constructor,
        "Symbol.hasInstance",
        capability_function(HostCapabilityKind::Custom(
            CapabilityName::CryptoDhHasInstance,
        )),
    )
    .expect("callable hasInstance");
    constructor
}

fn dh_group_constructor() -> Value {
    let constructor = dh_constructor();
    NODE_DH_GROUP_CONSTRUCTOR.with(|value| value.replace(Some(constructor.clone())));
    constructor
}

fn ecdh_constructor() -> Value {
    let constructor =
        capability_function(HostCapabilityKind::Custom(CapabilityName::CryptoCreateEcdh));
    quench_runtime::execute::set_callable_property(
        &constructor,
        "Symbol.hasInstance",
        capability_function(HostCapabilityKind::Custom(
            CapabilityName::CryptoDhHasInstance,
        )),
    )
    .expect("callable ECDH hasInstance");
    constructor
}

fn certificate_constructor() -> Value {
    let constructor = capability_function(HostCapabilityKind::Custom(
        CapabilityName::CryptoCertificateConstructor,
    ));
    let prototype = Value::object(vec![
        (
            "verifySpkac".into(),
            capability_function(HostCapabilityKind::Custom(
                CapabilityName::CryptoCertificateVerifySpkac,
            )),
        ),
        (
            "exportPublicKey".into(),
            capability_function(HostCapabilityKind::Custom(
                CapabilityName::CryptoCertificateExportPublicKey,
            )),
        ),
        (
            "exportChallenge".into(),
            capability_function(HostCapabilityKind::Custom(
                CapabilityName::CryptoCertificateExportChallenge,
            )),
        ),
    ]);
    quench_runtime::execute::set_callable_property(&constructor, "prototype", prototype)
        .expect("callable certificate prototype");
    for (name, capability) in [
        ("verifySpkac", CapabilityName::CryptoCertificateVerifySpkac),
        (
            "exportPublicKey",
            CapabilityName::CryptoCertificateExportPublicKey,
        ),
        (
            "exportChallenge",
            CapabilityName::CryptoCertificateExportChallenge,
        ),
    ] {
        quench_runtime::execute::set_callable_property(
            &constructor,
            name,
            capability_function(HostCapabilityKind::Custom(capability)),
        )
        .expect("callable certificate method");
    }
    quench_runtime::execute::set_callable_property(
        &constructor,
        "Symbol.hasInstance",
        capability_function(HostCapabilityKind::Custom(
            CapabilityName::CryptoCertificateHasInstance,
        )),
    )
    .expect("callable certificate hasInstance");
    constructor
}

fn basename(arguments: &[Value]) -> Result<Value, VmError> {
    let Some(Value::String(path)) = arguments.first() else {
        return Err(VmError::EvalError("path.basename expects a string".into()));
    };
    let mut value = Path::new(path)
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or(path)
        .to_string();
    if let Some(suffix) = arguments.get(1) {
        let Value::String(suffix) = suffix else {
            return Err(VmError::Thrown(fs_error(
                "ERR_INVALID_ARG_TYPE",
                "suffix must be a string",
            )));
        };
        if value.ends_with(suffix) {
            value.truncate(value.len() - suffix.len());
        }
    }
    Ok(Value::String(value.into()))
}
