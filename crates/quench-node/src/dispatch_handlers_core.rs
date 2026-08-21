
// ---- events ----
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
    if matches!(args.first(), Some(Value::Number(_))) {
        crate::modules::buffer::alloc(state, args)
    } else {
        crate::modules::buffer_from::from(state, args)
    }
}

// ---- tty ----
pub fn tty_isatty(
    state: &Rc<RefCell<HostState>>,
    _receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    crate::modules::tty::isatty(state, args)
}

// ---- process ----
pub fn crypto_random_bytes(
    state: &Rc<RefCell<HostState>>, _receiver: Option<&Value>, args: &[Value],
) -> Result<Value, VmError> { crate::modules::crypto::random_bytes(state, args) }
pub fn crypto_random_fill_sync(
    state: &Rc<RefCell<HostState>>, _receiver: Option<&Value>, args: &[Value],
) -> Result<Value, VmError> { crate::modules::crypto::random_fill_sync(state, args) }
pub fn crypto_unsupported(
    state: &Rc<RefCell<HostState>>, _receiver: Option<&Value>, args: &[Value],
) -> Result<Value, VmError> { crate::modules::crypto::unsupported(state, args) }
pub fn crypto_create_hash(
    s: &Rc<RefCell<HostState>>, _r: Option<&Value>, a: &[Value],
) -> Result<Value, VmError> { crate::modules::crypto::create_hash(s, a) }
pub fn crypto_create_hmac(
    s: &Rc<RefCell<HostState>>, _r: Option<&Value>, a: &[Value],
) -> Result<Value, VmError> { crate::modules::crypto::create_hmac(s, a) }
pub fn crypto_timing_safe_equal(
    s: &Rc<RefCell<HostState>>, _r: Option<&Value>, a: &[Value],
) -> Result<Value, VmError> { crate::modules::crypto::timing_safe_equal(s, a) }
pub fn crypto_random_uuid(
    s: &Rc<RefCell<HostState>>, _r: Option<&Value>, a: &[Value],
) -> Result<Value, VmError> { crate::modules::crypto::random_uuid(s, a) }
pub fn crypto_random_int(
    s: &Rc<RefCell<HostState>>, _r: Option<&Value>, a: &[Value],
) -> Result<Value, VmError> { crate::modules::crypto::random_int(s, a) }
pub fn crypto_get_hashes(
    s: &Rc<RefCell<HostState>>, _r: Option<&Value>, a: &[Value],
) -> Result<Value, VmError> { crate::modules::crypto::get_hashes(s, a) }
pub fn crypto_get_ciphers(
    s: &Rc<RefCell<HostState>>, _r: Option<&Value>, a: &[Value],
) -> Result<Value, VmError> { crate::modules::crypto::unsupported(s, a) }
pub fn crypto_hash_update(
    s: &Rc<RefCell<HostState>>, r: Option<&Value>, a: &[Value],
) -> Result<Value, VmError> { crate::modules::crypto::hash_update(s, r, a) }
pub fn crypto_hash_digest(
    s: &Rc<RefCell<HostState>>, r: Option<&Value>, a: &[Value],
) -> Result<Value, VmError> { crate::modules::crypto::hash_digest(s, r, a) }
pub fn crypto_hmac_update(
    s: &Rc<RefCell<HostState>>, r: Option<&Value>, a: &[Value],
) -> Result<Value, VmError> { crate::modules::crypto::hash_update(s, r, a) }
pub fn crypto_hmac_digest(
    s: &Rc<RefCell<HostState>>, r: Option<&Value>, a: &[Value],
) -> Result<Value, VmError> { crate::modules::crypto::hash_digest(s, r, a) }

pub fn process_exit(
    state: &Rc<RefCell<HostState>>,
    _receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    crate::modules::process::exit(state, args)
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
pub fn process_getuid(
    state: &Rc<RefCell<HostState>>,
    _receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    crate::modules::process::getuid(state, args)
}
pub fn process_getgid(
    state: &Rc<RefCell<HostState>>,
    _receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    crate::modules::process::getgid(state, args)
}
pub fn process_binding(
    state: &Rc<RefCell<HostState>>,
    _receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    crate::modules::process::binding(state, args)
}

pub fn process_active_resources(
    state: &Rc<RefCell<HostState>>,
    _receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    crate::modules::process::active_resources_info(state, args)
}

pub fn process_report(
    state: &Rc<RefCell<HostState>>,
    _receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    crate::modules::process::report(state, args)
}

// ---- os ----
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
    crate::modules::os::homedir(state, args)
}
pub fn os_eol(
    _state: &Rc<RefCell<HostState>>,
    _receiver: Option<&Value>,
    _args: &[Value],
) -> Result<Value, VmError> {
    Ok(Value::String(crate::modules::os::eol()))
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

// ---- dns ----
pub fn dns_lookup(
    state: &Rc<RefCell<HostState>>,
    _receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    crate::modules::dns::lookup(state, args)
}
pub fn dns_resolve4(
    state: &Rc<RefCell<HostState>>,
    _receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    crate::modules::dns::resolve4(state, args)
}

