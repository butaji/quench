fn url_pattern_construct(arguments: &[Value]) -> Result<Value, VmError> {
    if let Some(value) = arguments.get(1) {
        let string_overload = matches!(arguments.first(), Some(Value::String(_)))
            && matches!(value, Value::String(_));
        if !string_overload && !matches!(value, Value::Object(_) | Value::Undefined | Value::Null) {
            return Err(VmError::Thrown(fs_error(
                "ERR_INVALID_ARG_TYPE",
                "optionsFlags",
            )));
        }
    }
    if let Some(Value::Object(flags)) = arguments.get(1) {
        quench_runtime::execute::get_property_result(&Value::Object(flags.clone()), "ignoreCase")?;
    }
    let options = match arguments.first() {
        None | Some(Value::Null) | Some(Value::Undefined) => None,
        Some(Value::Object(options)) => Some(Value::Object(options.clone())),
        Some(Value::String(value)) => {
            let parsed =
                url::Url::parse(value).map_err(|error| VmError::EvalError(error.to_string()))?;
            Some(Value::object(vec![
                (
                    "hostname".into(),
                    Value::String(parsed.host_str().unwrap_or_default().into()),
                ),
                ("protocol".into(), Value::String(parsed.scheme().into())),
                ("pathname".into(), Value::String(parsed.path().into())),
            ]))
        }
        _ => return Err(VmError::Thrown(fs_error("ERR_INVALID_ARG_TYPE", "options"))),
    };
    let property = |name: &str| {
        options
            .as_ref()
            .and_then(|options| quench_runtime::execute::get_property_result(options, name).ok())
            .filter(|value| !matches!(value, Value::Undefined))
            .unwrap_or_else(|| Value::String("*".into()))
    };
    let pattern = Value::object(
        [
            "protocol", "username", "password", "hostname", "port", "pathname", "search", "hash",
        ]
        .into_iter()
        .map(|name| (name.into(), property(name)))
        .collect(),
    );
    let pattern = quench_runtime::execute::set_property(
        pattern,
        "exec",
        capability_function(HostCapabilityKind::Custom(CapabilityName::UrlPatternExec)),
    );
    let pattern = quench_runtime::execute::set_property(
        pattern,
        "test",
        capability_function(HostCapabilityKind::Custom(CapabilityName::UrlPatternTest)),
    );
    Ok(pattern)
}

fn url_pattern_exec(receiver: Option<&Value>, arguments: &[Value]) -> Result<Value, VmError> {
    if matches!(arguments.first(), Some(Value::Null)) {
        return Err(VmError::Thrown(fs_error(
            "ERR_OPERATION_FAILED",
            "URLPattern test failed",
        )));
    }
    if matches!(arguments.get(1), Some(Value::Null)) {
        return Ok(Value::Null);
    }
    let input = arguments.first().map(safe_value_string).unwrap_or_default();
    if input.is_empty() {
        return Ok(Value::Null);
    }
    let parsed = url::Url::parse(&input).map_err(|error| VmError::EvalError(error.to_string()))?;
    let pathname = parsed.path().to_owned();
    let groups = if let Some(Value::String(pattern)) = receiver
        .and_then(|value| quench_runtime::execute::get_property_result(value, "pathname").ok())
    {
        pattern
            .strip_prefix("/:")
            .and_then(|name| name.strip_suffix(""))
            .map(|name| {
                let value = pathname.trim_start_matches('/');
                Value::object(vec![(name.into(), Value::String(value.into()))])
            })
            .unwrap_or_else(|| Value::object(vec![]))
    } else {
        Value::object(vec![])
    };
    let component = |input: Value, groups: Value| {
        Value::object(vec![("input".into(), input), ("groups".into(), groups)])
    };
    Ok(Value::object(vec![
        (
            "inputs".into(),
            quench_runtime::host_api::array(vec![Value::String(input.into())]),
        ),
        (
            "protocol".into(),
            component(Value::String(parsed.scheme().into()), Value::object(vec![])),
        ),
        (
            "username".into(),
            component(
                Value::String(parsed.username().into()),
                Value::object(vec![]),
            ),
        ),
        (
            "password".into(),
            component(
                Value::String(parsed.password().unwrap_or_default().into()),
                Value::object(vec![]),
            ),
        ),
        (
            "hostname".into(),
            component(
                Value::String(parsed.host_str().unwrap_or_default().into()),
                Value::object(vec![]),
            ),
        ),
        (
            "port".into(),
            component(
                Value::String(
                    parsed
                        .port()
                        .map_or(String::new(), |port| port.to_string())
                        .into(),
                ),
                Value::object(vec![]),
            ),
        ),
        (
            "pathname".into(),
            component(Value::String(pathname.into()), groups),
        ),
        (
            "search".into(),
            component(
                Value::String(
                    parsed
                        .query()
                        .map_or(String::new(), |query| format!("?{query}"))
                        .into(),
                ),
                Value::object(vec![]),
            ),
        ),
        (
            "hash".into(),
            component(
                Value::String(
                    parsed
                        .fragment()
                        .map_or(String::new(), |fragment| format!("#{fragment}"))
                        .into(),
                ),
                Value::object(vec![]),
            ),
        ),
    ]))
}

fn url_pattern_test(receiver: Option<&Value>, arguments: &[Value]) -> Result<Value, VmError> {
    if arguments.is_empty() || matches!(arguments.first(), Some(Value::Undefined)) {
        return Ok(Value::Boolean(true));
    }
    if matches!(arguments.first(), Some(Value::Null)) {
        return Err(VmError::Thrown(fs_error(
            "ERR_OPERATION_FAILED",
            "URLPattern test failed",
        )));
    }
    if matches!(arguments.get(1), Some(Value::Null)) {
        return Ok(Value::Boolean(false));
    }
    url_pattern_exec(receiver, arguments).map(|value| Value::Boolean(!matches!(value, Value::Null)))
}

fn url_search_params_construct(arguments: &[Value]) -> Result<Value, VmError> {
    let size = match arguments.first() {
        Some(Value::String(value)) => value
            .trim_start_matches('?')
            .split('&')
            .filter(|pair| !pair.is_empty())
            .count(),
        None | Some(Value::Undefined) => 0,
        _ => 0,
    };
    Ok(Value::object(vec![(
        "size".into(),
        Value::Number(size as f64),
    )]))
}
