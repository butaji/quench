fn internal_binding(arguments: &[Value]) -> Result<Value, VmError> {
    let Some(Value::String(name)) = arguments.first() else {
        return Err(VmError::Thrown(fs_error(
            "ERR_INVALID_ARG_TYPE",
            "binding name must be a string",
        )));
    };
    if name == "util" {
        return Ok(util_types_module());
    }
    if name == "os" {
        let binding = quench_runtime::host_api::object(vec![(
            "getHomeDirectory".into(),
            capability_function(HostCapabilityKind::Custom(
                CapabilityName::InternalOsGetHomeDirectory,
            )),
        )]);
        NODE_OS_BINDING.with(|stored| stored.replace(Some(binding.clone())));
        return Ok(binding);
    }
    if [
        "buffer",
        "cares_wrap",
        "constants",
        "contextify",
        "fs",
        "fs_event_wrap",
        "icu",
        "inspector",
        "js_stream",
        "natives",
        "os",
        "pipe_wrap",
        "spawn_sync",
        "stream_wrap",
        "tcp_wrap",
        "tls_wrap",
        "tty_wrap",
        "udp_wrap",
        "uv",
        "zlib",
    ]
    .contains(&name.as_str())
    {
        return Ok(quench_runtime::host_api::object(vec![]));
    }
    Err(VmError::Thrown(fs_error(
        "ERR_UNKNOWN_BUILTIN_MODULE",
        "Unknown internal builtin module",
    )))
}

fn util_types_module() -> Value {
    NODE_UTIL_TYPES.with(|module| {
        module
            .borrow_mut()
            .get_or_insert_with(|| {
                let predicate = capability_function(HostCapabilityKind::Custom(
                    CapabilityName::InternalArrayBufferViewHasBuffer,
                ));
                let names = [
                    "isAnyArrayBuffer",
                    "isArrayBuffer",
                    "isArrayBufferView",
                    "isAsyncFunction",
                    "isDataView",
                    "isDate",
                    "isExternal",
                    "isMap",
                    "isMapIterator",
                    "isNativeError",
                    "isPromise",
                    "isRegExp",
                    "isSet",
                    "isSetIterator",
                    "isTypedArray",
                    "isUint8Array",
                ];
                quench_runtime::host_api::object(
                    names
                        .into_iter()
                        .map(|name| (name.into(), predicate.clone()))
                        .collect(),
                )
            })
            .clone()
    })
}

fn internal_util_sleep(arguments: &[Value]) -> Result<Value, VmError> {
    let Some(Value::Number(value)) = arguments.first() else {
        return Err(VmError::Thrown(fs_error(
            "ERR_INVALID_ARG_TYPE",
            "delay must be of type number",
        )));
    };
    if !value.is_finite() || value.fract() != 0.0 || *value < 0.0 || *value > u32::MAX as f64 {
        return Err(VmError::Thrown(fs_error(
            "ERR_OUT_OF_RANGE",
            "delay out of range",
        )));
    }
    Ok(Value::Undefined)
}

fn internal_util_emit_experimental_warning(arguments: &[Value]) -> Result<Value, VmError> {
    let Some(Value::String(feature)) = arguments.first() else {
        return Err(VmError::Thrown(fs_error(
            "ERR_INVALID_ARG_TYPE",
            "feature must be a string",
        )));
    };
    let is_new = NODE_EXPERIMENTAL_WARNINGS.with(|warnings| {
        let mut warnings = warnings.borrow_mut();
        if warnings.iter().any(|value| value == feature) {
            false
        } else {
            warnings.push(feature.to_string());
            true
        }
    });
    if !is_new {
        return Ok(Value::Undefined);
    }
    let warning = Value::object(vec![
        ("name".into(), Value::String("ExperimentalWarning".into())),
        (
            "message".into(),
            Value::String(format!("{feature} is an experimental feature").into()),
        ),
    ]);
    process_emit(&[Value::String("warning".into()), warning])?;
    Ok(Value::Undefined)
}

fn internal_view_has_buffer(arguments: &[Value]) -> Result<Value, VmError> {
    let length = quench_runtime::execute::get_property_result(
        arguments.first().ok_or(VmError::NotCallable)?,
        "byteLength",
    )
    .ok();
    Ok(Value::Boolean(
        matches!(length, Some(Value::Number(value)) if value >= 64.0),
    ))
}

fn stream_promises_module() -> Value {
    NODE_STREAM_PROMISES.with(|module| {
        let mut module = module.borrow_mut();
        if module.is_none() {
            *module = Some(quench_runtime::host_api::object(vec![
                (
                    "pipeline".into(),
                    capability_function(HostCapabilityKind::Custom(CapabilityName::StreamPipeline)),
                ),
                (
                    "finished".into(),
                    capability_function(HostCapabilityKind::Custom(CapabilityName::StreamFinished)),
                ),
            ]));
        }
        module.as_ref().unwrap().clone()
    })
}

fn timers_promises_module() -> Value {
    NODE_TIMERS_PROMISES.with(|module| {
        let mut module = module.borrow_mut();
        if module.is_none() {
            *module = Some(quench_runtime::host_api::object(vec![]));
        }
        module.as_ref().unwrap().clone()
    })
}

fn util_module() -> Value {
    let default_options =
        quench_runtime::host_api::object(vec![("numericSeparator".into(), Value::Boolean(false))]);
    let format = quench_runtime::execute::set_property(
        capability_function(HostCapabilityKind::Custom(CapabilityName::UtilFormat)),
        "defaultOptions",
        default_options.clone(),
    );
    let inspect = quench_runtime::execute::set_property(
        capability_function(HostCapabilityKind::Custom(CapabilityName::UtilInspect)),
        "defaultOptions",
        default_options,
    );
    let types = util_types_module();
    quench_runtime::host_api::object(vec![
        ("format".into(), format),
        ("inspect".into(), inspect),
        (
            "formatWithOptions".into(),
            capability_function(HostCapabilityKind::Custom(
                CapabilityName::UtilFormatWithOptions,
            )),
        ),
        (
            "promisify".into(),
            capability_function(HostCapabilityKind::Custom(CapabilityName::UtilPromisify)),
        ),
        (
            "deprecate".into(),
            capability_function(HostCapabilityKind::Custom(CapabilityName::UtilDeprecate)),
        ),
        (
            "parseEnv".into(),
            capability_function(HostCapabilityKind::Custom(CapabilityName::UtilParseEnv)),
        ),
        (
            "getSystemErrorName".into(),
            capability_function(HostCapabilityKind::Custom(
                CapabilityName::UtilSystemErrorName,
            )),
        ),
        (
            "getSystemErrorMessage".into(),
            capability_function(HostCapabilityKind::Custom(
                CapabilityName::UtilSystemErrorMessage,
            )),
        ),
        (
            "_exceptionWithHostPort".into(),
            capability_function(HostCapabilityKind::Custom(
                CapabilityName::UtilExceptionWithHostPort,
            )),
        ),
        (
            "_errnoException".into(),
            capability_function(HostCapabilityKind::Custom(
                CapabilityName::UtilExceptionWithHostPort,
            )),
        ),
        (
            "getSystemErrorMap".into(),
            capability_function(HostCapabilityKind::Custom(
                CapabilityName::UtilSystemErrorMap,
            )),
        ),
        ("types".into(), types),
        (
            "getCallSites".into(),
            capability_function(HostCapabilityKind::Custom(CapabilityName::UtilGetCallSites)),
        ),
        (
            "TextEncoder".into(),
            capability_function(HostCapabilityKind::Custom(
                CapabilityName::TextEncoderConstructor,
            )),
        ),
        (
            "TextDecoder".into(),
            capability_function(HostCapabilityKind::Custom(
                CapabilityName::TextDecoderConstructor,
            )),
        ),
    ])
}

fn util_parse_env(arguments: &[Value]) -> Result<Value, VmError> {
    let Some(Value::String(source)) = arguments.first() else {
        return Err(VmError::Thrown(fs_error(
            "ERR_INVALID_ARG_TYPE",
            "str must be a string",
        )));
    };
    let mut values = Vec::new();
    let mut pending: Option<(String, String)> = None;
    for line in source.lines() {
        let line = line.trim();
        if let Some((key, value)) = pending.as_mut() {
            value.push('\n');
            value.push_str(line);
            if line.ends_with('"') {
                values.push((
                    key.clone(),
                    Value::String(value.trim_matches('"').replace("\\n", "\n").into()),
                ));
                pending = None;
            }
            continue;
        }
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let Some((key, mut value)) = line.split_once('=') else {
            continue;
        };
        let key = key.trim().to_owned();
        value = value.trim();
        if value.starts_with('"') && value.matches('"').count() % 2 == 1 {
            pending = Some((key, value.to_owned()));
            continue;
        }
        if value.len() >= 2 && value.starts_with('"') && value.ends_with('"') {
            value = &value[1..value.len() - 1];
        } else if let Some((uncommented, _)) = value.split_once('#') {
            value = uncommented.trim_end();
        }
        values.push((key, Value::String(value.replace("\\n", "\n").into())));
    }
    let mut unique = HashMap::new();
    for (key, value) in values {
        unique.insert(key, value);
    }
    let mut properties = vec![("\0prototype".into(), Value::Null)];
    properties.extend(unique);
    Ok(Value::object(properties))
}

fn util_system_error_name(arguments: &[Value]) -> Result<Value, VmError> {
    let Some(Value::Number(errno)) = arguments.first() else {
        return Err(VmError::Thrown(fs_error(
            "ERR_INVALID_ARG_TYPE",
            "code must be a number",
        )));
    };
    let name = match *errno as i32 {
        -2 => "ENOENT".to_owned(),
        -17 => "EEXIST".to_owned(),
        -32 => "EPIPE".to_owned(),
        -105 => "ENOBUFS".to_owned(),
        _ => format!("Unknown system error {errno}"),
    };
    Ok(Value::String(name.into()))
}

fn util_system_error_message(arguments: &[Value]) -> Result<Value, VmError> {
    util_system_error_name(arguments)
}

fn util_exception_with_host_port(arguments: &[Value]) -> Result<Value, VmError> {
    let errno = match arguments.first() {
        Some(Value::Number(value)) => *value as i32,
        _ => 0,
    };
    let syscall = arguments.get(1).map(safe_value_string).unwrap_or_default();
    let address = arguments.get(2).map(safe_value_string).unwrap_or_default();
    let port = match arguments.get(3) {
        Some(Value::Number(value)) if *value != 0.0 => Some(*value as u32),
        _ => None,
    };
    let info = arguments.get(4).map(safe_value_string);
    let code = if errno == -2 { "ENOENT" } else { "UNKNOWN" };
    let mut message = format!("{syscall} {code} {address}");
    if let Some(port) = port {
        message.push_str(&format!(":{port} - Local"));
        if let Some(info) = info {
            message.push_str(&format!(" ({info})"));
        }
    }
    let mut error = fs_error(code, &message);
    error = quench_runtime::execute::set_property(error, "errno", Value::Number(errno as f64));
    error = quench_runtime::execute::set_property(error, "address", Value::String(address.into()));
    if let Some(port) = port {
        error = quench_runtime::execute::set_property(error, "port", Value::Number(port as f64));
    }
    Ok(error)
}

fn util_system_error_map_get(arguments: &[Value]) -> Result<Value, VmError> {
    let errno = match arguments.first() {
        Some(Value::Number(value)) => *value as i32,
        _ => 0,
    };
    let name = match errno {
        -2 => "ENOENT",
        -17 => "EEXIST",
        -32 => "EPIPE",
        -105 => "ENOBUFS",
        _ => return Ok(Value::Undefined),
    };
    Ok(quench_runtime::host_api::array(vec![
        Value::String(name.into()),
        Value::String(name.into()),
    ]))
}
