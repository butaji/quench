fn os_priority_error(code: &str, message: &str) -> VmError {
    VmError::Thrown(Value::object(vec![
        ("name".into(), Value::String("TypeError".into())),
        ("code".into(), Value::String(code.into())),
        ("message".into(), Value::String(message.into())),
    ]))
}
fn os_pid(arguments: &[Value]) -> Result<u32, VmError> {
    let pid = arguments.first().cloned().unwrap_or(Value::Number(0.0));
    if matches!(pid, Value::Undefined) {
        return Ok(0);
    }
    let Value::Number(pid) = pid else {
        return Err(os_priority_error(
            "ERR_INVALID_ARG_TYPE",
            "The \"pid\" argument must be of type number.",
        ));
    };
    if !pid.is_finite() || pid.fract() != 0.0 || pid < 0.0 || pid > u32::MAX as f64 {
        return Err(VmError::Thrown(Value::object(vec![
            ("name".into(), Value::String("RangeError".into())),
            ("code".into(), Value::String("ERR_OUT_OF_RANGE".into())),
            (
                "message".into(),
                Value::String("The value of \"pid\" is out of range.".into()),
            ),
        ])));
    }
    Ok(pid as u32)
}
pub fn os_get_priority(
    _state: &Rc<RefCell<HostState>>,
    _receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    if matches!(args.first(), Some(Value::Number(value)) if *value == -1.0) {
        return Err(VmError::Thrown(Value::object(vec![
            ("name".into(), Value::String("SystemError".into())),
            ("code".into(), Value::String("ERR_SYSTEM_ERROR".into())),
            (
                "message".into(),
                Value::String("A system error occurred: uv_os_getpriority returned ESRCH".into()),
            ),
        ])));
    }
    let _ = os_pid(args)?;
    Ok(Value::Number(OS_PRIORITY.with(Cell::get) as f64))
}
pub fn os_set_priority(
    _state: &Rc<RefCell<HostState>>,
    _receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    let priority = if args.len() == 1 {
        args[0].clone()
    } else {
        let _ = os_pid(args)?;
        args.get(1).cloned().unwrap_or(Value::Number(0.0))
    };
    let Value::Number(priority) = priority else {
        return Err(os_priority_error(
            "ERR_INVALID_ARG_TYPE",
            "The \"priority\" argument must be of type number.",
        ));
    };
    if !priority.is_finite() || priority.fract() != 0.0 || priority < -20.0 || priority > 19.0 {
        return Err(VmError::Thrown(Value::object(vec![
            ("name".into(), Value::String("RangeError".into())),
            ("code".into(), Value::String("ERR_OUT_OF_RANGE".into())),
            (
                "message".into(),
                Value::String("The value of \"priority\" is out of range.".into()),
            ),
        ])));
    }
    OS_PRIORITY.with(|value| value.set(priority as i32));
    Ok(Value::Undefined)
}
pub fn tty_isatty(
    state: &Rc<RefCell<HostState>>,
    _receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    crate::modules::tty::isatty(state, args)
}
pub fn os_platform(
    _state: &Rc<RefCell<HostState>>,
    _receiver: Option<&Value>,
    _args: &[Value],
) -> Result<Value, VmError> {
    Ok(Value::String(crate::modules::os::platform()))
}
pub fn os_arch(
    _: &Rc<RefCell<HostState>>,
    _: Option<&Value>,
    _: &[Value],
) -> Result<Value, VmError> {
    Ok(Value::String(crate::modules::os::arch()))
}
pub fn os_type(
    _state: &Rc<RefCell<HostState>>,
    _receiver: Option<&Value>,
    _args: &[Value],
) -> Result<Value, VmError> {
    Ok(Value::String(crate::modules::os::type_str()))
}
pub fn os_release(
    _state: &Rc<RefCell<HostState>>,
    _receiver: Option<&Value>,
    _args: &[Value],
) -> Result<Value, VmError> {
    Ok(Value::String(crate::modules::os::release()))
}
pub fn os_cpus(
    state: &Rc<RefCell<HostState>>,
    _receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    crate::modules::os::cpus(state, args)
}
pub fn os_tmpdir(
    state: &Rc<RefCell<HostState>>,
    _receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    crate::modules::os::tmpdir(state, args)
}
pub fn os_homedir(
    state: &Rc<RefCell<HostState>>,
    _receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    if let Some(binding) = state.borrow().os_binding.clone() {
        let context = host_api::object(Vec::new());
        if let Ok(get_home) =
            quench_runtime::execute::get_property_result(&binding, "getHomeDirectory")
        {
            let _ = quench_runtime::execute::call(
                &get_home,
                &Value::Undefined,
                std::slice::from_ref(&context),
            );
            if let Ok(Value::String(syscall)) =
                quench_runtime::execute::get_property_result(&context, "syscall")
            {
                let code = quench_runtime::execute::get_property_result(&context, "code")
                    .unwrap_or(Value::Undefined);
                let message = quench_runtime::execute::get_property_result(&context, "message")
                    .unwrap_or(Value::Undefined);
                let text = |value: &Value| match value {
                    Value::String(value) => value.to_string(),
                    Value::Undefined => "undefined".to_string(),
                    value => format!("{value:?}"),
                };
                return Err(VmError::Thrown(host_api::object(vec![(
                    "message".into(),
                    Value::String(
                        format!(
                            "A system error occurred: {} returned {} ({})",
                            syscall,
                            text(&code),
                            text(&message)
                        )
                        .into(),
                    ),
                )])));
            }
        }
    }
    crate::modules::os::homedir(state, args)
}
pub fn os_eol(
    _state: &Rc<RefCell<HostState>>,
    _receiver: Option<&Value>,
    _args: &[Value],
) -> Result<Value, VmError> {
    Ok(Value::String(crate::modules::os::eol()))
}
pub fn os_endianness(
    state: &Rc<RefCell<HostState>>,
    _receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    crate::modules::os::endianness(state, args)
}
pub fn os_version(
    state: &Rc<RefCell<HostState>>,
    _receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    crate::modules::os::version(state, args)
}
pub fn os_machine(
    state: &Rc<RefCell<HostState>>,
    _receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    crate::modules::os::machine(state, args)
}
pub fn os_user_info(
    state: &Rc<RefCell<HostState>>,
    _receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    crate::modules::os::user_info(state, args)
}
pub fn os_uptime(
    state: &Rc<RefCell<HostState>>,
    _receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    crate::modules::os::uptime(state, args)
}
pub fn os_freemem(
    state: &Rc<RefCell<HostState>>,
    _receiver: Option<&Value>,
    _args: &[Value],
) -> Result<Value, VmError> {
    crate::modules::os::freemem(state, _args)
}
pub fn os_totalmem(
    state: &Rc<RefCell<HostState>>,
    _receiver: Option<&Value>,
    _args: &[Value],
) -> Result<Value, VmError> {
    crate::modules::os::totalmem(state, _args)
}
pub fn os_loadavg(
    state: &Rc<RefCell<HostState>>,
    _receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    crate::modules::os::loadavg(state, args)
}
pub fn os_network_interfaces(
    state: &Rc<RefCell<HostState>>,
    _receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    crate::modules::os::network_interfaces(state, args)
}
pub fn os_hostname(
    state: &Rc<RefCell<HostState>>,
    _receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    crate::modules::os::hostname(state, args)
}
