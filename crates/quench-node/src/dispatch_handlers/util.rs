fn value_text(value: &Value) -> String {
    match value {
        Value::String(value) => value.clone(),
        Value::Number(value) => value.to_string(),
        Value::Boolean(value) => value.to_string(),
        Value::Null => "null".into(),
        Value::Undefined => "undefined".into(),
        _ => "[object Object]".into(),
    }
}
pub fn util_format(
    _state: &Rc<RefCell<HostState>>,
    _receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    crate::modules::util::validate_json_arguments(args)?;
    Ok(Value::String(crate::modules::util::format(args)))
}
pub fn util_inspect(
    _state: &Rc<RefCell<HostState>>,
    _receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    let arg = args.first().cloned().unwrap_or(Value::Undefined);
    if matches!(
        execute::get_property(&arg, "\0source_text_module"),
        Value::Boolean(true)
    ) && args.get(1).is_some_and(|options| {
        matches!(
            execute::get_property(options, "depth"),
            Value::Number(value) if value < 0.0
        )
    }) {
        return Ok(Value::String("[SourceTextModule]".into()));
    }
    if matches!(execute::get_property(&arg, "Symbol.toStringTag"), Value::String(ref tag) if tag == "AbortController")
        && execute::has_own_property(&arg, "signal")
    {
        let depth = args.get(1).and_then(|options| {
            let value = if matches!(options, Value::Object(_) | Value::ObjectAlias(_)) {
                execute::get_property(options, "depth")
            } else {
                options.clone()
            };
            match value {
                Value::Null => Some(usize::MAX),
                Value::Number(n) if n.is_finite() && n >= 0.0 => Some(n as usize),
                _ => None,
            }
        });
        let signal = execute::get_property(&arg, "signal");
        let aborted = execute::get_property(&signal, "aborted");
        return Ok(Value::String(if depth.is_some_and(|value| value <= 1) {
            "AbortController { signal: [AbortSignal] }".into()
        } else {
            format!(
                "AbortController {{ signal: AbortSignal {{ aborted: {} }} }}",
                crate::modules::util::inspect(&aborted)
            )
        }));
    }
    if let (Value::Object(_), Some(options)) = (&arg, args.get(1)) {
        let depth = execute::get_property(options, "depth");
        let tag = execute::get_property(&arg, "Symbol.toStringTag");
        if matches!(depth, Value::Number(value) if value < 0.0)
            && matches!(tag, Value::String(ref value) if value == "Event" || value == "CustomEvent")
        {
            return Ok(tag);
        }
    }
    let depth = args
        .get(1)
        .filter(|options| matches!(options, Value::Object(_) | Value::ObjectAlias(_)))
        .or_else(|| args.get(2))
        .and_then(|options| {
            let options = if matches!(options, Value::Object(_) | Value::ObjectAlias(_)) {
                execute::get_property(options, "depth")
            } else {
                options.clone()
            };
            match options {
                Value::Null => Some(usize::MAX / 2),
                Value::Number(value) if value.is_finite() && value >= 0.0 => {
                    Some(value.floor() as usize + 1)
                }
                _ => None,
            }
        });
    let show_hidden = matches!(args.get(1), Some(Value::Boolean(true)))
        || args.get(1).is_some_and(|options| {
            matches!(
                execute::get_property(options, "showHidden"),
                Value::Boolean(true)
            )
        });
    let show_proxy =
        args.get(1).is_some_and(
            |options| match execute::get_property(options, "showProxy") {
                Value::Boolean(value) => value,
                Value::Number(value) => value != 0.0 && !value.is_nan(),
                _ => false,
            },
        );
    let colors = args.get(1).is_some_and(|options| {
        matches!(
            execute::get_property(options, "colors"),
            Value::Boolean(true)
        )
    });
    let break_length_one = args.get(1).is_some_and(|options| {
        matches!(
            execute::get_property(options, "breakLength"),
            Value::Number(value) if value <= 1.0
        )
    });
    if colors && break_length_one {
        if let Some(rendered) = crate::modules::util::inspect_proxy_colored(&arg) {
            return Ok(Value::String(rendered));
        }
    }
    if let Some(rendered) =
        crate::modules::util::inspect_proxy(&arg, depth.unwrap_or(3), show_proxy)
    {
        return Ok(Value::String(rendered));
    }
    let max_array_length = args
        .iter()
        .find(|value| matches!(value, Value::Object(_) | Value::ObjectAlias(_)))
        .and_then(
            |options| match execute::get_property(options, "maxArrayLength") {
                Value::Number(value) if value.is_finite() && value >= 0.0 => {
                    Some(value.floor() as usize)
                }
                _ => None,
            },
        );
    let getters = args.iter().any(|value| {
        matches!(value, Value::Object(_) | Value::ObjectAlias(_))
            && matches!(
                execute::get_property(value, "getters"),
                Value::Boolean(true)
            )
    });
    Ok(Value::String(match depth {
        Some(depth) => crate::modules::util::inspect_with_options_colors(
            &arg,
            depth,
            show_hidden,
            max_array_length,
            getters,
            colors,
        ),
        None if show_hidden || getters || max_array_length.is_some() => {
            crate::modules::util::inspect_with_options_colors(
                &arg,
                3,
                show_hidden,
                max_array_length,
                getters,
                colors,
            )
        }
        None => {
            crate::modules::util::inspect_with_options_colors(&arg, 3, false, None, false, colors)
        }
    }))
}
pub fn util_aborted(
    state: &Rc<RefCell<HostState>>,
    _receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    let rejected = |message: &str| {
        let promise = Rc::new(quench_runtime::value::PromiseData::new(
            quench_runtime::value::PromiseState::Pending,
        ));
        let error = host_api::object(vec![
            ("code".into(), Value::String("ERR_INVALID_ARG_TYPE".into())),
            ("name".into(), Value::String("TypeError".into())),
            ("message".into(), Value::String(message.into())),
        ]);
        quench_runtime::reject_promise(&promise, error);
        Value::Promise(promise)
    };
    let Some(signal) = args.first() else {
        return Ok(rejected("The signal argument must be an AbortSignal"));
    };
    let Some(resource) = args.get(1) else {
        return Ok(rejected("The resource argument must be an object"));
    };
    if !matches!(resource, Value::Object(_) | Value::ObjectAlias(_)) {
        return Ok(rejected("The resource argument must be an object"));
    }
    if !matches!(
        execute::get_property(signal, crate::modules::event_target::ABORT_SIGNAL_BRAND),
        Value::Boolean(true)
    ) {
        return Ok(rejected("The signal argument must be an AbortSignal"));
    }
    let promise = Rc::new(quench_runtime::value::PromiseData::new(
        quench_runtime::value::PromiseState::Pending,
    ));
    if execute::is_truthy(&execute::get_property(signal, "aborted")) {
        quench_runtime::resolve_promise(&promise, Value::Undefined);
        return Ok(Value::Promise(promise));
    }
    let callback = quench_runtime::host_api::bound_capability_with_arguments(
        quench_runtime::ops::HostCapabilityRef {
            realm: quench_runtime::ops::RealmId::ROOT,
            kind: quench_runtime::ops::HostCapabilityKind::Custom(
                crate::registry::SPEC_UTIL_ABORTED_RESOLVE.cap,
            ),
        },
        vec![
            Value::Promise(promise.clone()),
            Value::Number(GC_EPOCH.with(Cell::get) as f64),
        ],
    );
    crate::modules::event_target::add_event_listener(
        state,
        Some(signal),
        &[
            Value::String("abort".into()),
            callback,
            host_api::object(vec![("once".into(), Value::Boolean(true))]),
        ],
    )?;
    Ok(Value::Promise(promise))
}
pub fn util_aborted_resolve(
    _state: &Rc<RefCell<HostState>>,
    _receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    let captured = args.get(1).and_then(|value| match value {
        Value::Number(value) => Some(*value as u64),
        _ => None,
    });
    if captured != Some(GC_EPOCH.with(Cell::get)) {
        return Ok(Value::Undefined);
    }
    if let Some(Value::Promise(promise)) = args.first() {
        quench_runtime::resolve_promise(promise, Value::Undefined);
    }
    Ok(Value::Undefined)
}
pub fn util_parse_env(
    _state: &Rc<RefCell<HostState>>,
    _receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    crate::modules::util::parse_env(args)
}
pub fn util_promisify(
    state: &Rc<RefCell<HostState>>,
    _: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    let Some(original) = args.first().cloned() else {
        return Err(VmError::NotCallable);
    };
    if !quench_runtime::is_callable(&original) {
        return Err(VmError::NotCallable);
    }
    let custom_key = crate::modules::util::PROMISIFY_CUSTOM_KEY;
    if let Ok(custom) = execute::get_property_result(&original, custom_key) {
        if !matches!(custom, Value::Undefined) {
            if quench_runtime::is_callable(&custom) {
                // Node canonicalizes a custom promisifier by making the
                // returned function idempotent: promisify(custom) returns
                // the same identity. Keep this mutation on the existing
                // function rather than wrapping it again.
                let _ = execute::set_property_in_place(&custom, custom_key, custom.clone());
                return Ok(
                    match timer_promise_alias(&original).or_else(|| {
                        match execute::get_property(&custom, "name") {
                            Value::String(name) if name.ends_with("Promise") => Some("timer"),
                            _ => None,
                        }
                    }) {
                        Some(_) => match execute::get_property(&original, "name") {
                            Value::String(name) => execute::define_property(
                                custom.clone(),
                                "name",
                                host_api::object(vec![
                                    (
                                        "value".into(),
                                        Value::String(name.trim_end_matches("Promise").to_string()),
                                    ),
                                    ("configurable".into(), Value::Boolean(true)),
                                ]),
                            )
                            .unwrap_or(custom),
                            _ => custom,
                        },
                        None => custom,
                    },
                );
            }
            return Err(VmError::NotCallable);
        }
    }
    if let Some(name) = timer_promise_alias(&original) {
        let timers = crate::modules::require::require(
            state,
            &[Value::String("timers/promises".to_string())],
        );
        if let Ok(timers) = timers {
            let promise_api = execute::get_property(&timers, name);
            return Ok(match execute::get_property(&original, "name") {
                Value::String(name) => {
                    execute::set_property(promise_api, "name", Value::String(name))
                }
                _ => promise_api,
            });
        }
    }
    let wrapper = bound_custom(
        crate::registry::SPEC_UTIL_PROMISIFIED_CALL.cap,
        vec![original.clone()],
    );
    let wrapper = match execute::get_property(&original, "name") {
        Value::String(name) => execute::set_property(wrapper, "name", Value::String(name)),
        _ => wrapper,
    };
    let custom = wrapper.clone();
    if !execute::set_property_in_place(&wrapper, crate::modules::util::PROMISIFY_CUSTOM_KEY, custom)
    {
        return Err(VmError::NotCallable);
    }
    Ok(wrapper)
}
pub fn util_deprecate(
    _state: &Rc<RefCell<HostState>>,
    _receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    let Some(callback) = args.first().cloned() else {
        return Err(VmError::NotCallable);
    };
    if !quench_runtime::is_callable(&callback) {
        return Err(VmError::NotCallable);
    }
    if let Some(code) = args.get(2) {
        if !matches!(code, Value::String(_)) {
            let received = match code {
                Value::Null => " Received null".to_string(),
                Value::Boolean(value) => format!(" Received type boolean ({value})"),
                Value::Number(value) => format!(" Received type number ({value})"),
                Value::Object(_) | Value::ObjectAlias(_) => {
                    " Received an instance of Object".to_string()
                }
                _ => " Received an unsupported value".to_string(),
            };
            return Err(VmError::Thrown(quench_runtime::host_api::object(vec![
                ("code".into(), Value::String("ERR_INVALID_ARG_TYPE".into())),
                ("name".into(), Value::String("TypeError".into())),
                (
                    "message".into(),
                    Value::String(format!(
                        "The \"code\" argument must be of type string.{received}"
                    )),
                ),
            ])));
        }
    }
    let mut wrapper = bound_custom(
        crate::registry::SPEC_UTIL_DEPRECATED_CALL.cap,
        vec![
            callback.clone(),
            args.get(1).cloned().unwrap_or(Value::String(String::new())),
            args.get(2).cloned().unwrap_or(Value::Undefined),
        ],
    );
    let length = execute::get_property(&callback, "length");
    wrapper = execute::set_property(wrapper, "length", length);
    let modify = args
        .get(3)
        .map(|options| execute::get_property(options, "modifyPrototype"))
        .unwrap_or(Value::Undefined);
    if !matches!(modify, Value::Boolean(false)) {
        let prototype = execute::get_property(&callback, "prototype");
        wrapper = execute::set_property(wrapper, "prototype", prototype);
        wrapper = execute::set_prototype_of(&wrapper, &callback)?;
    } else {
        wrapper = execute::set_property(
            wrapper,
            "prototype",
            quench_runtime::host_api::object(Vec::new()),
        );
    }
    Ok(wrapper)
}
pub fn util_debuglog(
    _state: &Rc<RefCell<HostState>>,
    _receiver: Option<&Value>,
    _args: &[Value],
) -> Result<Value, VmError> {
    Ok(bound_custom(
        crate::registry::SPEC_UTIL_DEBUGLOG.cap,
        vec![],
    ))
}
pub fn util_deprecated_call(
    state: &Rc<RefCell<HostState>>,
    _receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    let Some(callback) = args.first() else {
        return Err(VmError::NotCallable);
    };
    let message = match args.get(1) {
        Some(Value::String(value)) => value.as_str(),
        _ => "",
    };
    let code = match args.get(2) {
        Some(Value::String(value)) => Some(value.as_str()),
        _ => None,
    };
    if crate::modules::process::mark_deprecation(state, callback, code) {
        crate::modules::process::emit_warning(state, "DeprecationWarning", message, code, false);
    }
    quench_runtime::execute::call(
        callback,
        &Value::Undefined,
        args.get(3..).unwrap_or_default(),
    )
}
pub fn util_system_error_name(
    _state: &Rc<RefCell<HostState>>,
    _receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    let Some(Value::Number(errno)) = args.first() else {
        return Err(VmError::NotCallable);
    };
    let name = match *errno as i32 {
        -2 => "ENOENT",
        -13 => "EACCES",
        -17 => "EEXIST",
        -32 => "EPIPE",
        -105 => "ENOBUFS",
        -110 => "ETIMEDOUT",
        _ => return Ok(Value::String(format!("Unknown system error {errno}"))),
    };
    Ok(Value::String(name.into()))
}
pub fn util_convert_signal_to_exit_code(
    _state: &Rc<RefCell<HostState>>,
    _receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    let Some(Value::String(signal)) = args.first() else {
        return Err(VmError::Thrown(host_api::object(vec![
            ("name".into(), Value::String("TypeError".into())),
            ("code".into(), Value::String("ERR_INVALID_ARG_VALUE".into())),
            (
                "message".into(),
                Value::String("The signal argument must be a valid signal name".into()),
            ),
        ])));
    };
    let number = match signal.as_str() {
        "SIGHUP" => 1,
        "SIGINT" => 2,
        "SIGABRT" => 6,
        "SIGKILL" => 9,
        "SIGTERM" => 15,
        _ => {
            return Err(VmError::Thrown(host_api::object(vec![
                ("name".into(), Value::String("TypeError".into())),
                ("code".into(), Value::String("ERR_INVALID_ARG_VALUE".into())),
                (
                    "message".into(),
                    Value::String("The signal argument must be a valid signal name".into()),
                ),
            ])))
        }
    };
    Ok(Value::Number((128 + number) as f64))
}
pub fn util_exception_with_host_port(
    _state: &Rc<RefCell<HostState>>,
    _receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    let errno = match args.first() {
        Some(Value::Number(value)) => *value,
        _ => 0.0,
    };
    let syscall = args.get(1).map(value_text).unwrap_or_default();
    let address = match args.get(2) {
        Some(Value::Null) | None => None,
        Some(value) => Some(value_text(value)),
    };
    let port = match args.get(3) {
        Some(Value::Number(value)) if *value != 0.0 => Some(*value as u32),
        _ => None,
    };
    let code = match errno as i32 {
        -2 => "ENOENT",
        -13 => "EACCES",
        -17 => "EEXIST",
        _ => "UNKNOWN",
    };
    let mut message = format!("{syscall} {code}");
    if let Some(address) = &address {
        message.push_str(&format!(" {address}"));
        if let Some(port) = port {
            message.push_str(&format!(":{port}"));
        }
    }
    if let Some(additional) = args.get(4) {
        message.push_str(&format!(" - Local ({})", value_text(additional)));
    }
    let mut properties = vec![
        (
            "\0prototype".into(),
            Value::Builtin(quench_runtime::ops::Builtin::ErrorPrototype),
        ),
        ("name".into(), Value::String("Error".into())),
        ("message".into(), Value::String(message)),
        ("stack".into(), Value::String("Error".into())),
        ("errno".into(), Value::Number(errno)),
        ("code".into(), Value::String(code.into())),
        ("syscall".into(), Value::String(syscall)),
    ];
    properties.push(("address".into(), address.map_or(Value::Null, Value::String)));
    if let Some(port) = port {
        properties.push(("port".into(), Value::Number(port as f64)));
    }
    Ok(Value::object(properties))
}
pub fn util_promisified_call(
    state: &Rc<RefCell<HostState>>,
    receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    let Some(original) = args.first() else {
        return Err(VmError::NotCallable);
    };
    let promise = Rc::new(quench_runtime::value::PromiseData::new(
        quench_runtime::value::PromiseState::Pending,
    ));
    let custom_args = quench_runtime::execute::get_property_result(
        original,
        crate::modules::util::PROMISIFY_CUSTOM_ARGS_KEY,
    )
    .unwrap_or(Value::Undefined);
    let callback = bound_custom(
        crate::registry::SPEC_UTIL_PROMISIFIED_CALLBACK.cap,
        vec![Value::Promise(Rc::clone(&promise)), custom_args],
    );
    let mut call_args = args.get(1..).unwrap_or_default().to_vec();
    call_args.push(callback);
    let receiver = receiver.cloned().unwrap_or(Value::Undefined);
    match quench_runtime::vm::call_value(original, &receiver, &call_args) {
        Ok(result) => {
            if matches!(result, Value::Object(_) | Value::ObjectAlias(_)) {
                let promise_value = Value::Promise(Rc::clone(&promise));
                let _ = execute::set_property(promise_value, "child", result.clone());
            }
            if matches!(result, Value::Promise(_)) {
                crate::modules::process::emit_warning(
                    state,
                    "DeprecationWarning",
                    "Calling promisify on a function that returns a Promise is likely a mistake.",
                    Some("DEP0174"),
                    false,
                );
            }
        }
        Err(VmError::Thrown(error)) => {
            // execFile's Node promisifier validates its AbortSignal before
            // creating the promise; preserve that synchronous TypeError.
            let is_exec_file = matches!(
                &original,
                Value::BoundFunction(bound)
                    if matches!(
                        bound.target,
                        Value::Builtin(quench_runtime::ops::Builtin::HostCapability(
                            quench_runtime::ops::HostCapabilityKind::Custom(0x1e03)
                        ))
                    )
            );
            if is_exec_file
                && matches!(
                    execute::get_property(&error, "code"),
                    Value::String(ref code) if code == "ERR_INVALID_ARG_TYPE"
                )
            {
                return Err(VmError::Thrown(error));
            }
            quench_runtime::reject_promise(&promise, error)
        }
        Err(_) => quench_runtime::reject_promise(&promise, Value::Undefined),
    }
    Ok(Value::Promise(promise))
}
pub fn util_promisified_callback(
    _: &Rc<RefCell<HostState>>,
    receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    let Value::Promise(promise) = args.first().cloned().unwrap_or(Value::Undefined) else {
        return Err(VmError::NotCallable);
    };
    let custom_args = args.get(1).cloned().unwrap_or(Value::Undefined);
    let error = args.get(2).cloned().unwrap_or(Value::Undefined);
    if !matches!(error, Value::Undefined | Value::Null) {
        quench_runtime::reject_promise(&promise, error);
    } else {
        let values = args.get(3..).unwrap_or_default();
        if let Value::Array(names) = custom_args {
            let mut properties = Vec::new();
            for index in 0..names.logical_len() {
                let key = quench_runtime::execute::get_property_result(
                    &Value::Array(names.clone()),
                    &index.to_string(),
                );
                let value = values.get(index).cloned().unwrap_or(Value::Undefined);
                if let Ok(Value::String(key)) = key {
                    properties.push((key, value));
                }
            }
            if !properties.is_empty() {
                quench_runtime::resolve_promise(&promise, host_api::object(properties));
                return Ok(Value::Undefined);
            }
        }
        let value = match values {
            [] => Value::Undefined,
            [value] => value.clone(),
            values => host_api::array(values.to_vec()),
        };
        quench_runtime::resolve_promise(&promise, value);
    }
    Ok(Value::Undefined)
}
fn number_display(value: &Value) -> String {
    match value {
        Value::Number(number) => number.to_string(),
        _ => "unknown".to_string(),
    }
}
pub fn util_get_call_sites(
    state: &Rc<RefCell<HostState>>,
    _receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    if args.len() > 2
        || args
            .get(1)
            .is_some_and(|options| !matches!(options, Value::Object(_) | Value::ObjectAlias(_)))
    {
        return Err(quench_runtime::execute::type_error(
            "The options argument must be an object",
        ));
    }
    let count = match args.first() {
        None | Some(Value::Undefined) | Some(Value::Object(_)) | Some(Value::ObjectAlias(_)) => 10,
        Some(Value::Number(value))
            if value.is_finite() && *value >= 1.0 && value.fract() == 0.0 =>
        {
            if *value > 200.0 {
                return Err(VmError::Thrown(Value::object(vec![
                    ("name".into(), Value::String("RangeError".into())),
                    ("code".into(), Value::String("ERR_OUT_OF_RANGE".into())),
                    (
                        "message".into(),
                        Value::String("The frame count must be between 1 and 200".into()),
                    ),
                ])));
            }
            *value as usize
        }
        Some(Value::Number(_)) => {
            return Err(VmError::Thrown(Value::object(vec![
                ("name".into(), Value::String("RangeError".into())),
                ("code".into(), Value::String("ERR_OUT_OF_RANGE".into())),
                (
                    "message".into(),
                    Value::String("The frame count must be an integer between 1 and 200".into()),
                ),
            ])));
        }
        Some(_) => {
            return Err(quench_runtime::execute::type_error(
                "The frame count must be an integer",
            ));
        }
    };
    let script_name = state
        .borrow()
        .process
        .argv
        .get(1)
        .cloned()
        .map(Value::String)
        .unwrap_or_else(|| Value::String(String::new()));
    Ok(quench_runtime::host_api::array(
        (0..count)
            .map(|_| {
                quench_runtime::host_api::object(vec![
                    ("scriptName".into(), script_name.clone()),
                    ("scriptId".into(), script_name.clone()),
                    ("lineNumber".into(), Value::Number(0.0)),
                    ("columnNumber".into(), Value::Number(0.0)),
                ])
            })
            .collect(),
    ))
}
fn format_output(value: &str, newline: bool) -> String {
    if newline {
        format!("{value}\n")
    } else {
        value.to_string()
    }
}
pub fn util_strip_vt(
    _state: &Rc<RefCell<HostState>>,
    _receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    let value = args.first().unwrap_or(&Value::Undefined);
    let text = match value {
        Value::String(text) => text,
        Value::StringUnits(units) => {
            return Ok(Value::String(
                crate::modules::util_strip::strip_vt_control_characters(&String::from_utf16_lossy(
                    units,
                )),
            ));
        }
        _ => {
            return Err(crate::modules::buffer_enc::invalid_arg_type(format!(
                "The \"str\" argument must be of type string.{}",
                crate::modules::util::invalid_arg_received(value)
            )));
        }
    };
    Ok(Value::String(
        crate::modules::util_strip::strip_vt_control_characters(text),
    ))
}
pub fn util_format_with_options(
    _state: &Rc<RefCell<HostState>>,
    _receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    let options = args.first().unwrap_or(&Value::Undefined);
    if !matches!(options, Value::Object(_) | Value::ObjectAlias(_)) {
        return Err(crate::modules::buffer_enc::invalid_arg_type(
            "The \"inspectOptions\" argument must be an object".into(),
        ));
    }
    let separator = quench_runtime::execute::is_truthy(&quench_runtime::execute::get_property(
        options,
        "numericSeparator",
    ));
    let colors = quench_runtime::execute::is_truthy(&quench_runtime::execute::get_property(
        options, "colors",
    ));
    let compact = !matches!(
        quench_runtime::execute::get_property(options, "compact"),
        Value::Undefined
    );
    crate::modules::util::validate_json_arguments(&args[1..])?;
    Ok(Value::String(crate::modules::util::format_with_options(
        &args[1..],
        separator,
        colors,
        compact,
    )))
}
pub fn util_is_deep_strict_equal(
    _state: &Rc<RefCell<HostState>>,
    _receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    let left = args.first().unwrap_or(&Value::Undefined);
    let right = args.get(1).unwrap_or(&Value::Undefined);
    let skip_prototype = match args.get(2) {
        Some(Value::Boolean(flag)) => *flag,
        Some(options) => quench_runtime::execute::is_truthy(
            &quench_runtime::execute::get_property(options, "skipPrototype"),
        ),
        None => false,
    };
    Ok(Value::Boolean(crate::modules::deep_equal::deep_equal_opts(
        left,
        right,
        true,
        skip_prototype,
    )?))
}
pub fn util_to_usv_string(
    _state: &Rc<RefCell<HostState>>,
    _receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    let value = args.first().unwrap_or(&Value::Undefined);
    let units = match value {
        Value::String(value) => value.encode_utf16().collect::<Vec<_>>(),
        Value::StringUnits(value) => value.iter().copied().collect::<Vec<_>>(),
        Value::Undefined => "undefined".encode_utf16().collect(),
        Value::Null => "null".encode_utf16().collect(),
        _ => return Ok(Value::String(format!("{value:?}"))),
    };
    let mut output = Vec::with_capacity(units.len());
    let mut index = 0;
    while index < units.len() {
        let unit = units[index];
        if (0xD800..=0xDBFF).contains(&unit) {
            if units
                .get(index + 1)
                .is_some_and(|next| (0xDC00..=0xDFFF).contains(next))
            {
                output.extend([unit, units[index + 1]]);
                index += 2;
                continue;
            }
            output.push(0xFFFD);
        } else if (0xDC00..=0xDFFF).contains(&unit) {
            output.push(0xFFFD);
        } else {
            output.push(unit);
        }
        index += 1;
    }
    Ok(quench_runtime::execute::string_from_units(output))
}
pub fn util_is_native_error(
    _state: &Rc<RefCell<HostState>>,
    _receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    let value = args.first().unwrap_or(&Value::Undefined);
    let native = matches!(
        quench_runtime::execute::get_property_result(value, "\0error_slot"),
        Ok(Value::Boolean(true))
    );
    Ok(Value::Boolean(native))
}
pub fn util_type_predicate(
    _state: &Rc<RefCell<HostState>>,
    _receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    let predicate = args
        .iter()
        .find_map(|value| match value {
            Value::String(name) => Some(name.as_str()),
            _ => None,
        })
        .unwrap_or("");
    Ok(Value::Boolean(crate::modules::util::type_predicate(
        predicate,
        args.iter()
            .find(|value| !matches!(value, Value::String(_)))
            .unwrap_or(&Value::Undefined),
    )))
}
