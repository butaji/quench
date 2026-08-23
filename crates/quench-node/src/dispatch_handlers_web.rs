// ---- web-compatible globals ----
pub fn structured_clone(
    _state: &Rc<RefCell<HostState>>,
    _receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    Ok(crate::modules::clone::deep_clone(
        args.first().cloned().unwrap_or(Value::Undefined),
    ))
}

pub fn fetch(
    _state: &Rc<RefCell<HostState>>,
    _receiver: Option<&Value>,
    _args: &[Value],
) -> Result<Value, VmError> {
    // `fetch()` is promise-based even when the transport is unavailable.  Returning
    // `undefined` here made `await fetch(...)` appear to succeed and caused the
    // subsequent `response.text()` access to fail with an unrelated TypeError.
    // Reject immediately with the same shape callers receive from other host
    // operations until a network transport is wired up.
    use quench_runtime::value::{PromiseData, PromiseState};
    let error = quench_runtime::host_api::object(vec![
        ("name".to_string(), Value::String("TypeError".to_string())),
        (
            "message".to_string(),
            Value::String("fetch is not available".to_string()),
        ),
    ]);
    Ok(Value::Promise(Rc::new(PromiseData::new(
        PromiseState::Rejected(error),
    ))))
}

pub fn abort_controller_new(
    _state: &Rc<RefCell<HostState>>,
    _args: &[Value],
) -> Result<Value, VmError> {
    let signal = quench_runtime::host_api::object(vec![
        ("aborted".to_string(), Value::Boolean(false)),
        (
            crate::modules::event_target::ABORT_SIGNAL_BRAND.to_string(),
            Value::Boolean(true),
        ),
    ]);
    Ok(quench_runtime::host_api::object(vec![(
        "signal".to_string(),
        signal,
    )]))
}

pub fn abort_signal_new(
    _state: &Rc<RefCell<HostState>>,
    _args: &[Value],
) -> Result<Value, VmError> {
    Ok(quench_runtime::host_api::object(vec![(
        "aborted".to_string(),
        Value::Boolean(false),
    )]))
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
pub fn process_emit(
    state: &Rc<RefCell<HostState>>,
    _receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    crate::modules::process::emit(state, args)
}

pub fn test_run(
    state: &Rc<RefCell<HostState>>,
    _receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    crate::modules::test::run(state, args)
}

pub fn util_strip_vt(
    _state: &Rc<RefCell<HostState>>,
    _receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    let text = quench_runtime::execute::to_js_string(args.first().unwrap_or(&Value::Undefined))?;
    Ok(Value::String(
        crate::modules::util_strip::strip_vt_control_characters(&text),
    ))
}

pub fn util_format_with_options(
    _state: &Rc<RefCell<HostState>>,
    _receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    let options = args.first().unwrap_or(&Value::Undefined);
    let separator = quench_runtime::execute::is_truthy(&quench_runtime::execute::get_property(
        options,
        "numericSeparator",
    ));
    Ok(Value::String(crate::modules::util::format_with_options(
        &args[1..],
        separator,
    )))
}

pub fn test_skip(
    state: &Rc<RefCell<HostState>>,
    _receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    crate::modules::test::skip(state, args)
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
pub fn dgram_create(state:&Rc<RefCell<HostState>>,_:Option<&Value>,args:&[Value])->Result<Value,VmError>{crate::modules::dgram::create_socket(state,args)}
pub fn dgram_bind(state:&Rc<RefCell<HostState>>,r:Option<&Value>,args:&[Value])->Result<Value,VmError>{crate::modules::dgram::bind(state,r,args)}
pub fn dgram_send(state:&Rc<RefCell<HostState>>,r:Option<&Value>,args:&[Value])->Result<Value,VmError>{crate::modules::dgram::send(state,r,args)}
pub fn dgram_close(state:&Rc<RefCell<HostState>>,r:Option<&Value>,args:&[Value])->Result<Value,VmError>{crate::modules::dgram::close(state,r,args)}
pub fn dgram_address(state:&Rc<RefCell<HostState>>,r:Option<&Value>,args:&[Value])->Result<Value,VmError>{crate::modules::dgram::address(state,r,args)}
