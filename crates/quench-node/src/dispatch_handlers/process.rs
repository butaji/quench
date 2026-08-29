pub fn process_exit(
    state: &Rc<RefCell<HostState>>,
    _receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    crate::modules::process::exit(state, args)
}
pub fn process_kill(
    state: &Rc<RefCell<HostState>>,
    _receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    crate::modules::process::kill(state, args)
}
pub fn process_cwd(
    state: &Rc<RefCell<HostState>>,
    _receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    crate::modules::process::cwd(state, args)
}
pub fn process_chdir(
    state: &Rc<RefCell<HostState>>,
    _receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    crate::modules::process::chdir(state, args)
}
pub fn process_next_tick(
    state: &Rc<RefCell<HostState>>,
    _receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    crate::modules::process::next_tick(state, args)
}
pub fn process_hrtime(
    state: &Rc<RefCell<HostState>>,
    _receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    crate::modules::process::hrtime(state, args)
}
pub fn process_hrtime_bigint(
    _state: &Rc<RefCell<HostState>>,
    _receiver: Option<&Value>,
    _args: &[Value],
) -> Result<Value, VmError> {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    Ok(Value::BigInt(nanos.to_string()))
}
pub fn process_cpu_usage(
    _state: &Rc<RefCell<HostState>>,
    _receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    if let Some(previous) = args.first() {
        if !matches!(previous, Value::Object(_)) {
            return Err(crate::modules::buffer_enc::invalid_arg_type(format!(
                "The \"prevValue\" argument must be of type object.{}",
                crate::modules::util::invalid_arg_received(previous)
            )));
        }
        for field in ["user", "system"] {
            let value = execute::get_property(previous, field);
            let Value::Number(number) = value else {
                return Err(crate::modules::buffer_enc::invalid_arg_type(format!(
                    "The \"prevValue.{field}\" property must be of type number.{}",
                    crate::modules::util::invalid_arg_received(&value)
                )));
            };
            if !number.is_finite() || number < 0.0 {
                return Err(VmError::Thrown(host_api::object(vec![
                    ("code".into(), Value::String("ERR_INVALID_ARG_VALUE".into())),
                    ("name".into(), Value::String("RangeError".into())),
                    (
                        "message".into(),
                        Value::String(format!(
                            "The property 'prevValue.{field}' is invalid. Received {}",
                            execute::number_to_js_string(number)
                        )),
                    ),
                ])));
            }
        }
    }
    Ok(quench_runtime::host_api::object(vec![
        ("user".into(), Value::Number(0.0)),
        ("system".into(), Value::Number(0.0)),
    ]))
}
pub fn process_uptime(
    _state: &Rc<RefCell<HostState>>,
    _receiver: Option<&Value>,
    _args: &[Value],
) -> Result<Value, VmError> {
    Ok(Value::Number(
        PROCESS_START
            .get_or_init(Instant::now)
            .elapsed()
            .as_secs_f64(),
    ))
}
pub fn process_available_memory(
    _state: &Rc<RefCell<HostState>>,
    _receiver: Option<&Value>,
    _args: &[Value],
) -> Result<Value, VmError> {
    Ok(Value::Number(f64::MAX))
}
pub fn process_constrained_memory(
    _state: &Rc<RefCell<HostState>>,
    _receiver: Option<&Value>,
    _args: &[Value],
) -> Result<Value, VmError> {
    Ok(Value::Number(f64::MAX))
}
pub fn process_umask(
    state: &Rc<RefCell<HostState>>,
    _receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    crate::modules::process::umask(state, args)
}
pub fn process_on(
    state: &Rc<RefCell<HostState>>,
    _receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    crate::modules::process::on(state, args)
}
pub fn process_once(
    state: &Rc<RefCell<HostState>>,
    _receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    crate::modules::process::once(state, args)
}
pub fn process_remove_listener(
    state: &Rc<RefCell<HostState>>,
    _receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    crate::modules::process::remove_listener(state, args)
}
pub fn process_remove_all_listeners(
    state: &Rc<RefCell<HostState>>,
    _receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    crate::modules::process::remove_all_listeners(state, args)
}
pub fn process_emit(
    state: &Rc<RefCell<HostState>>,
    _receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    crate::modules::process::emit(state, args)
}
pub fn process_emit_warning(
    state: &Rc<RefCell<HostState>>,
    _receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    let invalid = |message: &str| {
        VmError::Thrown(host_api::object(vec![
            ("name".into(), Value::String("TypeError".into())),
            ("code".into(), Value::String("ERR_INVALID_ARG_TYPE".into())),
            ("message".into(), Value::String(message.into())),
        ]))
    };
    let first = args.first().ok_or_else(|| {
        invalid("The \"warning\" argument must be of type string or an instance of Error.")
    })?;
    let error_object = matches!(first, Value::Object(_) | Value::ObjectAlias(_))
        && (matches!(
            quench_runtime::execute::get_property(first, "message"),
            Value::String(_)
        ) || matches!(
            quench_runtime::execute::get_property(first, "name"),
            Value::String(_)
        ));
    if !matches!(first, Value::String(_)) && !error_object {
        return Err(invalid(
            "The \"warning\" argument must be of type string or an instance of Error.",
        ));
    }
    for (index, value) in args.iter().enumerate().skip(1).take(2) {
        let valid = if index == 2 {
            matches!(value, Value::String(_) | Value::Undefined)
                || quench_runtime::is_callable(value)
        } else {
            matches!(
                value,
                Value::String(_) | Value::Object(_) | Value::ObjectAlias(_) | Value::Undefined
            ) || quench_runtime::is_callable(value)
        };
        if !valid {
            return Err(invalid(&format!(
                "The argument at position {index} must be a string or an object."
            )));
        }
    }
    let first = args.first().cloned().unwrap_or(Value::Undefined);
    let message = match &first {
        Value::Object(_) => match quench_runtime::execute::get_property(&first, "message") {
            Value::String(value) => value,
            _ => crate::modules::path::value_to_string(&first),
        },
        _ => crate::modules::path::value_to_string(&first),
    };
    let options = args.get(1).cloned().unwrap_or(Value::Undefined);
    let (name, code, detail) = match options {
        Value::String(name) => (name, None, None),
        Value::Object(_) => (
            match quench_runtime::execute::get_property(&options, "name") {
                Value::String(value) if !value.is_empty() => value,
                _ => "Warning".into(),
            },
            match quench_runtime::execute::get_property(&options, "code") {
                Value::String(value) => Some(value),
                _ => None,
            },
            match quench_runtime::execute::get_property(&options, "detail") {
                Value::String(value) => Some(value),
                _ => None,
            },
        ),
        _ => ("Warning".into(), None, None),
    };
    let global = quench_runtime::vm::current_global_object();
    let process = quench_runtime::execute::get_property(&global, "process");
    let no_deprecation = matches!(
        quench_runtime::execute::get_property(&process, "noDeprecation"),
        Value::Boolean(true)
    );
    if name == "DeprecationWarning" && no_deprecation {
        return Ok(Value::Undefined);
    }
    let throw_deprecation = name == "DeprecationWarning"
        && matches!(
            quench_runtime::execute::get_property(&process, "throwDeprecation"),
            Value::Boolean(true)
        );
    if throw_deprecation {
        let error = crate::host::namespace_object_from_pairs(vec![
            (
                "\0prototype".into(),
                Value::Builtin(quench_runtime::ops::Builtin::ErrorPrototype),
            ),
            ("name".into(), Value::String(name.clone())),
            ("message".into(), Value::String(message.clone())),
            ("stack".into(), Value::String(format!("{name}: {message}"))),
        ]);
        state.borrow_mut().event_loop.queue_microtask(
            crate::host::capability(crate::registry::SPEC_PROCESS_EMIT),
            vec![Value::String("uncaughtException".into()), error],
        );
        return Ok(Value::Undefined);
    }
    crate::modules::process::emit_warning_with_detail(
        state,
        &name,
        &message,
        code.as_deref(),
        detail.as_deref(),
        false,
    );
    Ok(Value::Undefined)
}
pub fn process_exit_code_get(
    state: &Rc<RefCell<HostState>>,
    _receiver: Option<&Value>,
    _args: &[Value],
) -> Result<Value, VmError> {
    Ok(Value::Number(
        state.borrow().process.exit_code.unwrap_or(0) as f64
    ))
}
pub fn process_exit_code_set(
    state: &Rc<RefCell<HostState>>,
    _receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    let value = args.first().cloned().unwrap_or(Value::Undefined);
    if matches!(value, Value::Undefined | Value::Null) {
        state.borrow_mut().process.exit_code = None;
        return Ok(Value::Undefined);
    }
    let code = match &value {
        Value::Number(number) if number.is_finite() && number.fract() == 0.0 => *number as i64,
        Value::String(text) if !text.is_empty() && text.chars().all(|c| c.is_ascii_digit()) => {
            text.parse::<i64>().unwrap_or(-1)
        }
        _ => -1,
    };
    if !(0..=255).contains(&code) {
        let received = match &value {
            Value::Number(number) => {
                let rendered = if number.is_nan() {
                    "NaN".to_string()
                } else if number.is_infinite() {
                    if number.is_sign_negative() {
                        "-Infinity"
                    } else {
                        "Infinity"
                    }
                    .to_string()
                } else {
                    number.to_string()
                };
                format!("Received {rendered}")
            }
            _ => crate::modules::util::invalid_arg_received(&value),
        };
        return Err(VmError::Thrown(host_api::object(vec![
            ("name".into(), Value::String("TypeError".into())),
            (
                "message".into(),
                Value::String(format!(
                    "The \"code\" argument must be of type number or string. {received}"
                )),
            ),
        ])));
    }
    state.borrow_mut().process.exit_code = Some(code as i32);
    Ok(Value::Undefined)
}
pub fn process_getuid(
    state: &Rc<RefCell<HostState>>,
    _: Option<&Value>,
    _: &[Value],
) -> Result<Value, VmError> {
    Ok(crate::modules::process::credential("uid"))
}
pub fn process_getgid(
    state: &Rc<RefCell<HostState>>,
    _: Option<&Value>,
    _: &[Value],
) -> Result<Value, VmError> {
    Ok(crate::modules::process::credential("gid"))
}
pub fn process_geteuid(
    state: &Rc<RefCell<HostState>>,
    _: Option<&Value>,
    _: &[Value],
) -> Result<Value, VmError> {
    Ok(crate::modules::process::credential("euid"))
}
pub fn process_getegid(
    state: &Rc<RefCell<HostState>>,
    _: Option<&Value>,
    _: &[Value],
) -> Result<Value, VmError> {
    Ok(crate::modules::process::credential("egid"))
}
pub fn process_setuid(
    _: &Rc<RefCell<HostState>>,
    _: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    crate::modules::process::set_credential("uid", args)
}
pub fn process_setgid(
    _: &Rc<RefCell<HostState>>,
    _: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    crate::modules::process::set_credential("gid", args)
}
pub fn process_seteuid(
    _: &Rc<RefCell<HostState>>,
    _: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    crate::modules::process::set_credential("uid", args)
}
pub fn process_setegid(
    _: &Rc<RefCell<HostState>>,
    _: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    crate::modules::process::set_credential("gid", args)
}
pub fn process_active_resources(
    state: &Rc<RefCell<HostState>>,
    _: Option<&Value>,
    _: &[Value],
) -> Result<Value, VmError> {
    Ok(crate::modules::process::active_resources_info(state))
}
