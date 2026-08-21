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
    state: &Rc<RefCell<HostState>>,
    _receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    crate::modules::crypto::random_bytes(state, args)
}
pub fn crypto_random_fill_sync(
    state: &Rc<RefCell<HostState>>,
    _receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    crate::modules::crypto::random_fill_sync(state, args)
}
pub fn crypto_unsupported(
    state: &Rc<RefCell<HostState>>,
    _receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    crate::modules::crypto::unsupported(state, args)
}
pub fn crypto_create_hash(
    s: &Rc<RefCell<HostState>>,
    _r: Option<&Value>,
    a: &[Value],
) -> Result<Value, VmError> {
    crate::modules::crypto::create_hash(s, a)
}
pub fn crypto_create_hmac(
    s: &Rc<RefCell<HostState>>,
    _r: Option<&Value>,
    a: &[Value],
) -> Result<Value, VmError> {
    crate::modules::crypto::create_hmac(s, a)
}
pub fn crypto_timing_safe_equal(
    s: &Rc<RefCell<HostState>>,
    _r: Option<&Value>,
    a: &[Value],
) -> Result<Value, VmError> {
    crate::modules::crypto::timing_safe_equal(s, a)
}
pub fn crypto_random_uuid(
    s: &Rc<RefCell<HostState>>,
    _r: Option<&Value>,
    a: &[Value],
) -> Result<Value, VmError> {
    crate::modules::crypto::random_uuid(s, a)
}
pub fn crypto_random_int(
    s: &Rc<RefCell<HostState>>,
    _r: Option<&Value>,
    a: &[Value],
) -> Result<Value, VmError> {
    crate::modules::crypto::random_int(s, a)
}
pub fn crypto_get_hashes(
    s: &Rc<RefCell<HostState>>,
    _r: Option<&Value>,
    a: &[Value],
) -> Result<Value, VmError> {
    crate::modules::crypto::get_hashes(s, a)
}
pub fn crypto_get_ciphers(
    s: &Rc<RefCell<HostState>>,
    _r: Option<&Value>,
    a: &[Value],
) -> Result<Value, VmError> {
    crate::modules::crypto::unsupported(s, a)
}
pub fn crypto_hash_update(
    s: &Rc<RefCell<HostState>>,
    r: Option<&Value>,
    a: &[Value],
) -> Result<Value, VmError> {
    crate::modules::crypto::hash_update(s, r, a)
}
pub fn crypto_hash_digest(
    s: &Rc<RefCell<HostState>>,
    r: Option<&Value>,
    a: &[Value],
) -> Result<Value, VmError> {
    crate::modules::crypto::hash_digest(s, r, a)
}
pub fn crypto_hmac_update(
    s: &Rc<RefCell<HostState>>,
    r: Option<&Value>,
    a: &[Value],
) -> Result<Value, VmError> {
    crate::modules::crypto::hash_update(s, r, a)
}
pub fn crypto_hmac_digest(
    s: &Rc<RefCell<HostState>>,
    r: Option<&Value>,
    a: &[Value],
) -> Result<Value, VmError> {
    crate::modules::crypto::hash_digest(s, r, a)
}
pub fn crypto_subtle_digest(s: &Rc<RefCell<HostState>>, _: Option<&Value>, a: &[Value]) -> Result<Value, VmError> { crate::modules::crypto::subtle_digest(s, a) }
pub fn crypto_subtle_import_key(s: &Rc<RefCell<HostState>>, _: Option<&Value>, a: &[Value]) -> Result<Value, VmError> { crate::modules::crypto::subtle_import_key(s, a) }
pub fn crypto_subtle_unsupported(s: &Rc<RefCell<HostState>>, _: Option<&Value>, a: &[Value]) -> Result<Value, VmError> { crate::modules::crypto::subtle_unsupported(s, a) }
pub fn crypto_subtle_derive_bits(s: &Rc<RefCell<HostState>>, _: Option<&Value>, a: &[Value]) -> Result<Value, VmError> { crate::modules::crypto::subtle_derive_bits(s, a) }
pub fn crypto_subtle_derive_key(s: &Rc<RefCell<HostState>>, _: Option<&Value>, a: &[Value]) -> Result<Value, VmError> { crate::modules::crypto::subtle_derive_key(s, a) }
pub fn crypto_subtle_sign(s: &Rc<RefCell<HostState>>, _: Option<&Value>, a: &[Value]) -> Result<Value, VmError> { crate::modules::crypto::subtle_sign(s, a) }
pub fn crypto_subtle_verify(s: &Rc<RefCell<HostState>>, _: Option<&Value>, a: &[Value]) -> Result<Value, VmError> { crate::modules::crypto::subtle_verify(s, a) }

pub fn process_exit(
    state: &Rc<RefCell<HostState>>,
    _: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    crate::modules::process::exit(state, args)
}
pub fn process_cwd(
    state: &Rc<RefCell<HostState>>,
    _: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    crate::modules::process::cwd(state, args)
}
pub fn process_chdir(
    state: &Rc<RefCell<HostState>>,
    _: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    crate::modules::process::chdir(state, args)
}
pub fn process_next_tick(
    state: &Rc<RefCell<HostState>>,
    _: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    crate::modules::process::next_tick(state, args)
}
pub fn process_hrtime(
    state: &Rc<RefCell<HostState>>,
    _: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    crate::modules::process::hrtime(state, args)
}
pub fn process_getuid(
    state: &Rc<RefCell<HostState>>,
    _: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    crate::modules::process::getuid(state, args)
}
pub fn process_getgid(
    state: &Rc<RefCell<HostState>>,
    _: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    crate::modules::process::getgid(state, args)
}
pub fn process_binding(
    state: &Rc<RefCell<HostState>>,
    _: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    crate::modules::process::binding(state, args)
}
pub fn process_active_resources(
    state: &Rc<RefCell<HostState>>,
    _: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    crate::modules::process::active_resources_info(state, args)
}
pub fn process_report(
    state: &Rc<RefCell<HostState>>,
    _: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    crate::modules::process::report(state, args)
}

// ---- os ----
pub fn os_platform(
    _: &Rc<RefCell<HostState>>,
    _: Option<&Value>,
    _: &[Value],
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
    _: &Rc<RefCell<HostState>>,
    _: Option<&Value>,
    _: &[Value],
) -> Result<Value, VmError> {
    Ok(Value::String(crate::modules::os::type_str()))
}
pub fn os_release(
    _: &Rc<RefCell<HostState>>,
    _: Option<&Value>,
    _: &[Value],
) -> Result<Value, VmError> {
    Ok(Value::String(crate::modules::os::release()))
}
pub fn os_cpus(
    s: &Rc<RefCell<HostState>>,
    _: Option<&Value>,
    a: &[Value],
) -> Result<Value, VmError> {
    crate::modules::os::cpus(s, a)
}
pub fn os_tmpdir(
    s: &Rc<RefCell<HostState>>,
    _: Option<&Value>,
    a: &[Value],
) -> Result<Value, VmError> {
    crate::modules::os::tmpdir(s, a)
}
pub fn os_homedir(
    s: &Rc<RefCell<HostState>>,
    _: Option<&Value>,
    a: &[Value],
) -> Result<Value, VmError> {
    crate::modules::os::homedir(s, a)
}
pub fn os_eol(
    _: &Rc<RefCell<HostState>>,
    _: Option<&Value>,
    _: &[Value],
) -> Result<Value, VmError> {
    Ok(Value::String(crate::modules::os::eol()))
}
pub fn os_uptime(
    s: &Rc<RefCell<HostState>>,
    _: Option<&Value>,
    a: &[Value],
) -> Result<Value, VmError> {
    crate::modules::os::uptime(s, a)
}
pub fn os_freemem(
    s: &Rc<RefCell<HostState>>,
    _: Option<&Value>,
    a: &[Value],
) -> Result<Value, VmError> {
    crate::modules::os::freemem(s, a)
}
pub fn os_totalmem(
    s: &Rc<RefCell<HostState>>,
    _: Option<&Value>,
    a: &[Value],
) -> Result<Value, VmError> {
    crate::modules::os::totalmem(s, a)
}
pub fn os_loadavg(
    s: &Rc<RefCell<HostState>>,
    _: Option<&Value>,
    a: &[Value],
) -> Result<Value, VmError> {
    crate::modules::os::loadavg(s, a)
}
pub fn os_network_interfaces(
    s: &Rc<RefCell<HostState>>,
    _: Option<&Value>,
    a: &[Value],
) -> Result<Value, VmError> {
    crate::modules::os::network_interfaces(s, a)
}
pub fn os_hostname(
    s: &Rc<RefCell<HostState>>,
    _: Option<&Value>,
    a: &[Value],
) -> Result<Value, VmError> {
    crate::modules::os::hostname(s, a)
}

// ---- dns ----
pub fn dns_lookup(
    s: &Rc<RefCell<HostState>>,
    _: Option<&Value>,
    a: &[Value],
) -> Result<Value, VmError> {
    crate::modules::dns::lookup(s, a)
}
pub fn dns_resolve4(
    s: &Rc<RefCell<HostState>>,
    _: Option<&Value>,
    a: &[Value],
) -> Result<Value, VmError> {
    crate::modules::dns::resolve4(s, a)
}
pub fn dns_promise_lookup(
    s: &Rc<RefCell<HostState>>,
    _: Option<&Value>,
    a: &[Value],
) -> Result<Value, VmError> {
    crate::modules::dns::promise_lookup(s, a)
}
pub fn dns_promise_resolve4(
    s: &Rc<RefCell<HostState>>,
    _: Option<&Value>,
    a: &[Value],
) -> Result<Value, VmError> {
    crate::modules::dns::promise_resolve4(s, a)
}
