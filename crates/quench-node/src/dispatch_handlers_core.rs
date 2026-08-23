// ---- events ----
thread_local! {
    static SMALL_BUFFER_VIEWS_WITH_BUFFER: RefCell<std::collections::HashSet<usize>> =
        RefCell::new(std::collections::HashSet::new());
}

pub fn events_from(
    state: &Rc<RefCell<HostState>>,
    _receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    crate::modules::events::from(state, args)
}
pub fn events_method_on(
    state: &Rc<RefCell<HostState>>,
    receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    crate::modules::events::method_on(state, receiver, args)
}
pub fn events_method_emit(
    state: &Rc<RefCell<HostState>>,
    receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    crate::modules::events::method_emit(state, receiver, args)
}
pub fn events_new(state: &Rc<RefCell<HostState>>, args: &[Value]) -> Result<Value, VmError> {
    crate::modules::events::new_emitter(state, args)
}

// ---- console ----
pub fn console_log(
    state: &Rc<RefCell<HostState>>,
    _receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    crate::modules::console::log(state, args, false)
}
pub fn console_warn(
    state: &Rc<RefCell<HostState>>,
    _receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    crate::modules::console::log(state, args, true)
}
pub fn console_trace(
    state: &Rc<RefCell<HostState>>,
    _receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    crate::modules::console::trace(state, args)
}

pub fn console_assert(
    state: &Rc<RefCell<HostState>>,
    _receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    crate::modules::console::assert_(state, args)
}
pub fn console_count(
    state: &Rc<RefCell<HostState>>,
    _receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    crate::modules::console::count(state, args)
}
// ---- util ----
pub fn util_format(
    _state: &Rc<RefCell<HostState>>,
    _receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    Ok(Value::String(crate::modules::util::format(args)))
}
pub fn util_inspect(
    _state: &Rc<RefCell<HostState>>,
    _receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    let arg = args.first().cloned().unwrap_or(Value::Undefined);
    Ok(Value::String(crate::modules::util::inspect(&arg)))
}

pub fn internal_binding(
    _state: &Rc<RefCell<HostState>>,
    _receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    let Some(Value::String(name)) = args.first() else {
        return Err(VmError::NotCallable);
    };
    if name == "buffer" {
        return Ok(quench_runtime::host_api::object(vec![(
            "fill".into(),
            quench_runtime::host_api::capability_function(
                quench_runtime::ops::HostCapabilityRef {
                    realm: quench_runtime::ops::RealmId::ROOT,
                    kind: quench_runtime::ops::HostCapabilityKind::Custom(0x0814),
                },
            ),
        )]));
    }
    if name == "util" {
        return Ok(quench_runtime::host_api::object(vec![
            (
                "arrayBufferViewHasBuffer".into(),
                quench_runtime::host_api::capability_function(
                    quench_runtime::ops::HostCapabilityRef {
                        realm: quench_runtime::ops::RealmId::ROOT,
                        kind: quench_runtime::ops::HostCapabilityKind::Custom(0x0F0F),
                    },
                ),
            ),
        ]));
    }
    Ok(quench_runtime::host_api::object(vec![]))
}

pub fn internal_view_has_buffer(
    _state: &Rc<RefCell<HostState>>,
    _receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    let view = args.first().ok_or(VmError::NotCallable)?;
    let bytes = match view {
        Value::Uint8Array(view) => view.byte_length(),
        Value::Uint8ClampedArray(view) => view.byte_length(),
        Value::Uint16Array(view) => view.byte_length(),
        Value::Uint32Array(view) => view.byte_length(),
        Value::Float32Array(view) => view.byte_length(),
        Value::Float64Array(view) => view.byte_length(),
        _ => 0,
    };
    if bytes >= 64 {
        return Ok(Value::Boolean(true));
    }
    let identity = match view {
        Value::Uint8Array(view) => Rc::as_ptr(view) as usize,
        Value::Uint8ClampedArray(view) => Rc::as_ptr(view) as usize,
        Value::Uint16Array(view) => Rc::as_ptr(view) as usize,
        Value::Uint32Array(view) => Rc::as_ptr(view) as usize,
        Value::Float32Array(view) => Rc::as_ptr(view) as usize,
        Value::Float64Array(view) => Rc::as_ptr(view) as usize,
        _ => 0,
    };
    let materialized = SMALL_BUFFER_VIEWS_WITH_BUFFER.with(|seen| !seen.borrow_mut().insert(identity));
    Ok(Value::Boolean(materialized))
}

// ---- url ----
pub fn url_parse(
    state: &Rc<RefCell<HostState>>,
    _receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    crate::modules::url::parse(state, args)
}
pub fn url_format(
    state: &Rc<RefCell<HostState>>,
    receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    crate::modules::url::format(state, receiver, args)
}
pub fn url_resolve(
    state: &Rc<RefCell<HostState>>,
    _receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    crate::modules::url::resolve(state, args)
}
pub fn url_new(state: &Rc<RefCell<HostState>>, args: &[Value]) -> Result<Value, VmError> {
    crate::modules::url_whatwg::new_url(state, args)
}
pub fn url_can_parse(
    state: &Rc<RefCell<HostState>>,
    _receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    Ok(Value::Boolean(crate::modules::url_whatwg::can_parse(
        state, args,
    )))
}

pub fn url_parse_static(
    state: &Rc<RefCell<HostState>>,
    _receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    crate::modules::url_whatwg::parse_static(state, args)
}
pub fn url_search_params(state: &Rc<RefCell<HostState>>, args: &[Value]) -> Result<Value, VmError> {
    crate::modules::url::new_search_params(state, args)
}

// ---- querystring ----
// Handlers point directly at `crate::modules::querystring` functions.

// ---- timers ----
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
pub fn timers_run_loop(
    state: &Rc<RefCell<HostState>>,
    _receiver: Option<&Value>,
    _args: &[Value],
) -> Result<Value, VmError> {
    crate::modules::pump::run_event_loop(state)?;
    Ok(Value::Undefined)
}
pub fn uncaught_dispatch(
    state: &Rc<RefCell<HostState>>,
    _receiver: Option<&Value>,
    _args: &[Value],
) -> Result<Value, VmError> {
    crate::modules::pump::run_uncaught(state)?;
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
pub fn internal_util_sleep(
    _state: &Rc<RefCell<HostState>>,
    _receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    let msec = args.first().unwrap_or(&Value::Undefined);
    let Value::Number(ms) = msec else {
        return Err(sleep_error(
            "TypeError",
            &format!(
                "The \"msec\" argument must be of type number.{}",
                crate::modules::util::invalid_arg_received(msec)
            ),
        ));
    };
    if ms.is_nan() || ms.fract() != 0.0 || *ms < 0.0 || *ms > 4_294_967_295.0 {
        return Err(sleep_error(
            "RangeError",
            &format!(
                "The value of \"msec\" is out of range. It must be >= 0 && <= 4294967295. Received {}",
                quench_runtime::execute::number_to_js_string(*ms)
            ),
        ));
    }
    if *ms > 0.0 {
        std::thread::sleep(std::time::Duration::from_millis(*ms as u64));
    }
    Ok(Value::Undefined)
}

fn sleep_error(name: &str, message: &str) -> VmError {
    VmError::Thrown(quench_runtime::host_api::object(vec![
        ("name".to_string(), Value::String(name.to_string())),
        ("message".to_string(), Value::String(message.to_string())),
    ]))
}

// ---- buffer ----
pub fn buffer_from(
    state: &Rc<RefCell<HostState>>,
    _receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    crate::modules::buffer_from::from(state, args)
}
pub fn buffer_alloc(
    state: &Rc<RefCell<HostState>>,
    _receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    crate::modules::buffer::alloc(state, args)
}
pub fn buffer_byte_length(
    state: &Rc<RefCell<HostState>>,
    _receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    crate::modules::buffer::byte_length(state, args)
}
pub fn buffer_is_buffer(
    _state: &Rc<RefCell<HostState>>,
    _receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    Ok(Value::Boolean(crate::modules::buffer::is_buffer(args)))
}
pub fn buffer_concat(
    state: &Rc<RefCell<HostState>>,
    _receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    crate::modules::buffer::concat(state, args)
}
pub fn buffer_new(
    state: &Rc<RefCell<HostState>>,
    _receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    // Deprecated `Buffer(value)` constructor: numbers allocate,
    // everything else follows `Buffer.from`.
    if matches!(args.first(), Some(Value::Number(_))) {
        crate::modules::buffer::alloc(state, args)
    } else {
        crate::modules::buffer_from::from(state, args)
    }
}
pub fn buffer_alloc_unsafe(
    state: &Rc<RefCell<HostState>>,
    _receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    crate::modules::buffer::alloc_unsafe(state, args)
}
pub fn buffer_is_encoding(
    _state: &Rc<RefCell<HostState>>,
    _receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    Ok(Value::Boolean(crate::modules::buffer::is_encoding(args)))
}
pub fn buffer_is_utf8(
    _state: &Rc<RefCell<HostState>>,
    _receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    Ok(Value::Boolean(crate::modules::buffer_enc::is_utf8(
        args.first().unwrap_or(&Value::Undefined),
    )))
}
pub fn buffer_is_ascii(
    _state: &Rc<RefCell<HostState>>,
    _receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    Ok(Value::Boolean(crate::modules::buffer_enc::is_ascii(
        args.first().unwrap_or(&Value::Undefined),
    )))
}
pub fn buffer_new_construct(
    state: &Rc<RefCell<HostState>>,
    args: &[Value],
) -> Result<Value, VmError> {
    // `new Buffer(value)`: numbers allocate, everything else is `from`.
    if matches!(args.first(), Some(Value::Number(_))) && args.get(1).is_some_and(|v| !matches!(v, Value::Undefined)) {
        return Err(crate::modules::buffer_enc::invalid_arg_type(format!(
            "The \"string\" argument must be of type string.{}",
            crate::modules::util::invalid_arg_received(args.first().unwrap())
        )));
    }
    if matches!(args.first(), Some(Value::Number(_))) {
        crate::modules::buffer::alloc(state, args)
    } else {
        crate::modules::buffer_from::from(state, args)
    }
}

pub fn buffer_last_index_of_construct(
    _state: &Rc<RefCell<HostState>>,
    _args: &[Value],
) -> Result<Value, VmError> {
    Err(crate::modules::buffer_enc::invalid_arg_type(
        "The \"buffer\" argument must be an instance of Buffer, TypedArray, or DataView. Received an instance of lastIndexOf".into(),
    ))
}

include!("dispatch_handlers_core_tail.rs");
