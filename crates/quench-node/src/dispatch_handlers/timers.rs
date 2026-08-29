fn timer_promise_alias(value: &Value) -> Option<&'static str> {
    let capability = match value {
        Value::Builtin(quench_runtime::ops::Builtin::HostCapability(
            quench_runtime::ops::HostCapabilityKind::Custom(cap),
        )) => Some(*cap),
        Value::BoundFunction(bound) => match bound.target {
            Value::Builtin(quench_runtime::ops::Builtin::HostCapability(
                quench_runtime::ops::HostCapabilityKind::Custom(cap),
            )) => Some(cap),
            _ => None,
        },
        _ => None,
    }?;
    match capability {
        0x0700 => Some("setTimeout"),
        0x0702 => Some("setInterval"),
        0x0704 => Some("setImmediate"),
        _ => None,
    }
}
pub fn timers_set_timeout(
    state: &Rc<RefCell<HostState>>,
    _receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    crate::modules::timers::set_timeout(state, args)
}
pub fn timers_clear_timeout(
    state: &Rc<RefCell<HostState>>,
    _receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    crate::modules::timers::clear_timeout(state, args)
}
pub fn timers_set_interval(
    state: &Rc<RefCell<HostState>>,
    _receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    crate::modules::timers::set_interval(state, args)
}
pub fn timers_clear_interval(
    state: &Rc<RefCell<HostState>>,
    _receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    crate::modules::timers::clear_timeout(state, args)
}
pub fn timers_set_immediate(
    state: &Rc<RefCell<HostState>>,
    _receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    crate::modules::timers::set_immediate(state, args)
}
pub fn timers_clear_immediate(
    state: &Rc<RefCell<HostState>>,
    _receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    crate::modules::timers::clear_immediate(state, args)
}
pub fn timers_tick(
    state: &Rc<RefCell<HostState>>,
    _receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    crate::modules::timers::tick(state, args)
}
pub fn timers_method_unref(
    state: &Rc<RefCell<HostState>>,
    receiver: Option<&Value>,
    _args: &[Value],
) -> Result<Value, VmError> {
    Ok(crate::modules::timers::method_unref(state, receiver))
}
pub fn timers_method_ref(
    state: &Rc<RefCell<HostState>>,
    receiver: Option<&Value>,
    _args: &[Value],
) -> Result<Value, VmError> {
    Ok(crate::modules::timers::method_ref(state, receiver))
}
pub fn timers_method_has_ref(
    state: &Rc<RefCell<HostState>>,
    receiver: Option<&Value>,
    _args: &[Value],
) -> Result<Value, VmError> {
    Ok(crate::modules::timers::method_has_ref(state, receiver))
}
pub fn timers_method_refresh(
    state: &Rc<RefCell<HostState>>,
    receiver: Option<&Value>,
    _args: &[Value],
) -> Result<Value, VmError> {
    Ok(crate::modules::timers::method_refresh(state, receiver))
}
pub fn timers_method_to_primitive(
    _: &Rc<RefCell<HostState>>,
    receiver: Option<&Value>,
    _: &[Value],
) -> Result<Value, VmError> {
    Ok(crate::modules::timers::method_to_primitive(receiver))
}
pub fn timers_run_loop(
    state: &Rc<RefCell<HostState>>,
    _receiver: Option<&Value>,
    _args: &[Value],
) -> Result<Value, VmError> {
    crate::modules::pump::run_event_loop(state)?;
    Ok(Value::Undefined)
}
pub fn timers_run_exit(
    state: &Rc<RefCell<HostState>>,
    _receiver: Option<&Value>,
    _args: &[Value],
) -> Result<Value, VmError> {
    crate::modules::pump::run_exit_handlers(state)?;
    Ok(Value::Undefined)
}
/// `internal/util.sleep(ms)` — synchronous sleep used by Node's own
/// tests to assert timer ordering.
pub fn timers_method_close(
    state: &Rc<RefCell<HostState>>,
    receiver: Option<&Value>,
    _args: &[Value],
) -> Result<Value, VmError> {
    Ok(crate::modules::timers::method_close(state, receiver))
}
pub fn timers_get_libuv_now(
    _state: &Rc<RefCell<HostState>>,
    _receiver: Option<&Value>,
    _args: &[Value],
) -> Result<Value, VmError> {
    Ok(Value::Number(crate::modules::timers::monotonic_ms() as f64))
}
pub fn timers_schedule(
    _: &Rc<RefCell<HostState>>,
    _: Option<&Value>,
    _: &[Value],
) -> Result<Value, VmError> {
    Ok(Value::Undefined)
}
pub fn timers_toggle_ref(
    _: &Rc<RefCell<HostState>>,
    _: Option<&Value>,
    _: &[Value],
) -> Result<Value, VmError> {
    Ok(Value::Undefined)
}
pub fn timers_toggle_immediate_ref(
    _: &Rc<RefCell<HostState>>,
    _: Option<&Value>,
    _: &[Value],
) -> Result<Value, VmError> {
    Ok(Value::Undefined)
}
fn sleep_error(name: &str, message: &str) -> VmError {
    VmError::Thrown(quench_runtime::host_api::object(vec![
        ("name".to_string(), Value::String(name.to_string())),
        ("message".to_string(), Value::String(message.to_string())),
    ]))
}
pub fn queue_microtask(
    state: &Rc<RefCell<HostState>>,
    _receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    let callback = args.first().cloned().unwrap_or(Value::Undefined);
    if !quench_runtime::is_callable(&callback) {
        return Err(quench_runtime::execute::type_error(
            "The \"callback\" argument must be of type function",
        ));
    }
    state
        .borrow_mut()
        .event_loop
        .queue_microtask(callback, vec![]);
    Ok(Value::Undefined)
}
