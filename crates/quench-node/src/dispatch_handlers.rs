//! Per-domain handler trampolines. Each trampoline adapts a
//! module-level function into the canonical `CallHandler`.
//! The handlers table is the single canonical place where the
//! capability id resolves to a Rust function.

use std::cell::RefCell;
use std::rc::Rc;

use quench_runtime::execute::VmError;
use quench_runtime::value::Value;

use crate::host::HostState;

pub type CallHandler = fn(&Rc<RefCell<HostState>>, &[Value]) -> Result<Value, VmError>;
pub type ConstructHandler = fn(&Rc<RefCell<HostState>>, &[Value]) -> Result<Value, VmError>;

// ---- events ----
pub fn events_from(state: &Rc<RefCell<HostState>>, args: &[Value]) -> Result<Value, VmError> {
    crate::modules::events::from(state, args)
}
pub fn events_method_on(state: &Rc<RefCell<HostState>>, args: &[Value]) -> Result<Value, VmError> {
    crate::modules::events::method_on(state, args)
}
pub fn events_method_emit(
    state: &Rc<RefCell<HostState>>,
    args: &[Value],
) -> Result<Value, VmError> {
    crate::modules::events::method_emit(state, args)
}
pub fn events_new(state: &Rc<RefCell<HostState>>, args: &[Value]) -> Result<Value, VmError> {
    crate::modules::events::new_emitter(state, args)
}

// ---- console ----
pub fn console_log(state: &Rc<RefCell<HostState>>, args: &[Value]) -> Result<Value, VmError> {
    crate::modules::console::log(state, args, false)
}
pub fn console_warn(state: &Rc<RefCell<HostState>>, args: &[Value]) -> Result<Value, VmError> {
    crate::modules::console::log(state, args, true)
}
pub fn console_trace(state: &Rc<RefCell<HostState>>, args: &[Value]) -> Result<Value, VmError> {
    crate::modules::console::trace(state, args)
}

// ---- util ----
pub fn util_format(_: &Rc<RefCell<HostState>>, args: &[Value]) -> Result<Value, VmError> {
    Ok(Value::String(crate::modules::util::format(args)))
}
pub fn util_inspect(_: &Rc<RefCell<HostState>>, args: &[Value]) -> Result<Value, VmError> {
    let arg = args.first().cloned().unwrap_or(Value::Undefined);
    Ok(Value::String(crate::modules::util::inspect(&arg)))
}

// ---- path ----
pub fn path_join(_: &Rc<RefCell<HostState>>, args: &[Value]) -> Result<Value, VmError> {
    Ok(Value::String(crate::modules::path::join(args)))
}
pub fn path_resolve(_: &Rc<RefCell<HostState>>, args: &[Value]) -> Result<Value, VmError> {
    Ok(Value::String(crate::modules::path::resolve(args)))
}
pub fn path_normalize(_: &Rc<RefCell<HostState>>, args: &[Value]) -> Result<Value, VmError> {
    Ok(Value::String(crate::modules::path::normalize(args)))
}
pub fn path_dirname(_: &Rc<RefCell<HostState>>, args: &[Value]) -> Result<Value, VmError> {
    Ok(Value::String(crate::modules::path::dirname(args)))
}
pub fn path_basename(_: &Rc<RefCell<HostState>>, args: &[Value]) -> Result<Value, VmError> {
    Ok(Value::String(crate::modules::path::basename(args)))
}
pub fn path_extname(_: &Rc<RefCell<HostState>>, args: &[Value]) -> Result<Value, VmError> {
    Ok(Value::String(crate::modules::path::extname(args)))
}
pub fn path_is_absolute(_: &Rc<RefCell<HostState>>, args: &[Value]) -> Result<Value, VmError> {
    Ok(Value::Boolean(crate::modules::path::is_absolute(args)))
}

// ---- url ----
pub fn url_parse(state: &Rc<RefCell<HostState>>, args: &[Value]) -> Result<Value, VmError> {
    crate::modules::url::parse(state, args)
}
pub fn url_format(_: &Rc<RefCell<HostState>>, args: &[Value]) -> Result<Value, VmError> {
    Ok(Value::String(crate::modules::url::format(args)))
}
pub fn url_resolve(state: &Rc<RefCell<HostState>>, args: &[Value]) -> Result<Value, VmError> {
    crate::modules::url::resolve(state, args)
}
pub fn url_new(state: &Rc<RefCell<HostState>>, args: &[Value]) -> Result<Value, VmError> {
    crate::modules::url::new_url(state, args)
}
pub fn url_search_params(state: &Rc<RefCell<HostState>>, args: &[Value]) -> Result<Value, VmError> {
    crate::modules::url::new_search_params(state, args)
}

// ---- querystring ----
pub fn qs_parse(state: &Rc<RefCell<HostState>>, args: &[Value]) -> Result<Value, VmError> {
    crate::modules::querystring::parse(state, args)
}
pub fn qs_stringify(_: &Rc<RefCell<HostState>>, args: &[Value]) -> Result<Value, VmError> {
    Ok(Value::String(crate::modules::querystring::stringify(args)))
}
pub fn qs_escape(_: &Rc<RefCell<HostState>>, args: &[Value]) -> Result<Value, VmError> {
    crate::modules::querystring::escape(args).map_err(|_| VmError::NotCallable)
}
pub fn qs_unescape(_: &Rc<RefCell<HostState>>, args: &[Value]) -> Result<Value, VmError> {
    crate::modules::querystring::unescape(args).map_err(|_| VmError::NotCallable)
}

// ---- timers ----
pub fn timers_set_timeout(
    state: &Rc<RefCell<HostState>>,
    args: &[Value],
) -> Result<Value, VmError> {
    crate::modules::timers::set_timeout(state, args)
}
pub fn timers_clear_timeout(
    state: &Rc<RefCell<HostState>>,
    args: &[Value],
) -> Result<Value, VmError> {
    crate::modules::timers::clear_timeout(state, args)
}
pub fn timers_set_interval(
    state: &Rc<RefCell<HostState>>,
    args: &[Value],
) -> Result<Value, VmError> {
    crate::modules::timers::set_interval(state, args)
}
pub fn timers_clear_interval(
    state: &Rc<RefCell<HostState>>,
    args: &[Value],
) -> Result<Value, VmError> {
    crate::modules::timers::clear_interval(state, args)
}
pub fn timers_set_immediate(
    state: &Rc<RefCell<HostState>>,
    args: &[Value],
) -> Result<Value, VmError> {
    crate::modules::timers::set_immediate(state, args)
}
pub fn timers_clear_immediate(
    state: &Rc<RefCell<HostState>>,
    args: &[Value],
) -> Result<Value, VmError> {
    crate::modules::timers::clear_immediate(state, args)
}
pub fn timers_tick(state: &Rc<RefCell<HostState>>, args: &[Value]) -> Result<Value, VmError> {
    crate::modules::timers::tick(state, args)
}

// ---- buffer ----
pub fn buffer_from(state: &Rc<RefCell<HostState>>, args: &[Value]) -> Result<Value, VmError> {
    crate::modules::buffer::from(state, args)
}
pub fn buffer_alloc(state: &Rc<RefCell<HostState>>, args: &[Value]) -> Result<Value, VmError> {
    crate::modules::buffer::alloc(state, args)
}
pub fn buffer_byte_length(
    state: &Rc<RefCell<HostState>>,
    args: &[Value],
) -> Result<Value, VmError> {
    crate::modules::buffer::byte_length(state, args)
}
pub fn buffer_is_buffer(_: &Rc<RefCell<HostState>>, args: &[Value]) -> Result<Value, VmError> {
    Ok(Value::Boolean(crate::modules::buffer::is_buffer(args)))
}
pub fn buffer_concat(state: &Rc<RefCell<HostState>>, args: &[Value]) -> Result<Value, VmError> {
    crate::modules::buffer::concat(state, args)
}
pub fn buffer_new(state: &Rc<RefCell<HostState>>, args: &[Value]) -> Result<Value, VmError> {
    if matches!(args.first(), Some(Value::String(_))) {
        crate::modules::buffer::from(state, args)
    } else {
        crate::modules::buffer::alloc(state, args)
    }
}

// ---- tty ----
pub fn tty_isatty(state: &Rc<RefCell<HostState>>, args: &[Value]) -> Result<Value, VmError> {
    crate::modules::tty::isatty(state, args)
}

// ---- process ----
pub fn process_exit(state: &Rc<RefCell<HostState>>, args: &[Value]) -> Result<Value, VmError> {
    crate::modules::process::exit(state, args)
}
pub fn process_cwd(state: &Rc<RefCell<HostState>>, args: &[Value]) -> Result<Value, VmError> {
    crate::modules::process::cwd(state, args)
}
pub fn process_chdir(state: &Rc<RefCell<HostState>>, args: &[Value]) -> Result<Value, VmError> {
    crate::modules::process::chdir(state, args)
}
pub fn process_next_tick(state: &Rc<RefCell<HostState>>, args: &[Value]) -> Result<Value, VmError> {
    crate::modules::process::next_tick(state, args)
}
pub fn process_hrtime(state: &Rc<RefCell<HostState>>, args: &[Value]) -> Result<Value, VmError> {
    crate::modules::process::hrtime(state, args)
}

// ---- os ----
pub fn os_platform(_: &Rc<RefCell<HostState>>, _: &[Value]) -> Result<Value, VmError> {
    Ok(Value::String(crate::modules::os::platform()))
}
pub fn os_arch(_: &Rc<RefCell<HostState>>, _: &[Value]) -> Result<Value, VmError> {
    Ok(Value::String(crate::modules::os::arch()))
}
pub fn os_type(_: &Rc<RefCell<HostState>>, _: &[Value]) -> Result<Value, VmError> {
    Ok(Value::String(crate::modules::os::type_str()))
}
pub fn os_release(_: &Rc<RefCell<HostState>>, _: &[Value]) -> Result<Value, VmError> {
    Ok(Value::String(crate::modules::os::release()))
}
pub fn os_cpus(state: &Rc<RefCell<HostState>>, args: &[Value]) -> Result<Value, VmError> {
    crate::modules::os::cpus(state, args)
}
pub fn os_tmpdir(state: &Rc<RefCell<HostState>>, args: &[Value]) -> Result<Value, VmError> {
    crate::modules::os::tmpdir(state, args)
}
pub fn os_homedir(state: &Rc<RefCell<HostState>>, args: &[Value]) -> Result<Value, VmError> {
    crate::modules::os::homedir(state, args)
}
pub fn os_eol(_: &Rc<RefCell<HostState>>, _: &[Value]) -> Result<Value, VmError> {
    Ok(Value::String(crate::modules::os::eol()))
}
pub fn os_uptime(state: &Rc<RefCell<HostState>>, args: &[Value]) -> Result<Value, VmError> {
    crate::modules::os::uptime(state, args)
}
pub fn os_freemem(_: &Rc<RefCell<HostState>>, _: &[Value]) -> Result<Value, VmError> {
    Ok(Value::Number(crate::modules::os::freemem()))
}
pub fn os_totalmem(_: &Rc<RefCell<HostState>>, _: &[Value]) -> Result<Value, VmError> {
    Ok(Value::Number(crate::modules::os::totalmem()))
}
pub fn os_loadavg(state: &Rc<RefCell<HostState>>, args: &[Value]) -> Result<Value, VmError> {
    crate::modules::os::loadavg(state, args)
}
pub fn os_network_interfaces(
    state: &Rc<RefCell<HostState>>,
    args: &[Value],
) -> Result<Value, VmError> {
    crate::modules::os::network_interfaces(state, args)
}
pub fn os_hostname(state: &Rc<RefCell<HostState>>, args: &[Value]) -> Result<Value, VmError> {
    crate::modules::os::hostname(state, args)
}

// ---- dns ----
pub fn dns_lookup(state: &Rc<RefCell<HostState>>, args: &[Value]) -> Result<Value, VmError> {
    crate::modules::dns::lookup(state, args)
}
pub fn dns_resolve4(state: &Rc<RefCell<HostState>>, args: &[Value]) -> Result<Value, VmError> {
    crate::modules::dns::resolve4(state, args)
}

// ---- fs ----
pub fn fs_read_file(state: &Rc<RefCell<HostState>>, args: &[Value]) -> Result<Value, VmError> {
    crate::modules::fs::read_file(state, args)
}
pub fn fs_write_file(state: &Rc<RefCell<HostState>>, args: &[Value]) -> Result<Value, VmError> {
    crate::modules::fs::write_file(state, args)
}
pub fn fs_stat(state: &Rc<RefCell<HostState>>, args: &[Value]) -> Result<Value, VmError> {
    crate::modules::fs::stat(state, args)
}
pub fn fs_readdir(state: &Rc<RefCell<HostState>>, args: &[Value]) -> Result<Value, VmError> {
    crate::modules::fs::readdir(state, args)
}
pub fn fs_exists(state: &Rc<RefCell<HostState>>, args: &[Value]) -> Result<Value, VmError> {
    crate::modules::fs::exists(state, args)
}
pub fn fs_mkdir(state: &Rc<RefCell<HostState>>, args: &[Value]) -> Result<Value, VmError> {
    crate::modules::fs::mkdir(state, args)
}
pub fn fs_unlink(state: &Rc<RefCell<HostState>>, args: &[Value]) -> Result<Value, VmError> {
    crate::modules::fs::unlink(state, args)
}
pub fn fs_read_file_sync(state: &Rc<RefCell<HostState>>, args: &[Value]) -> Result<Value, VmError> {
    crate::modules::fs::read_file_sync(state, args)
}
pub fn fs_write_file_sync(
    state: &Rc<RefCell<HostState>>,
    args: &[Value],
) -> Result<Value, VmError> {
    crate::modules::fs::write_file_sync(state, args)
}
pub fn fs_stat_sync(state: &Rc<RefCell<HostState>>, args: &[Value]) -> Result<Value, VmError> {
    crate::modules::fs::stat_sync(state, args)
}
pub fn fs_readdir_sync(state: &Rc<RefCell<HostState>>, args: &[Value]) -> Result<Value, VmError> {
    crate::modules::fs::readdir_sync(state, args)
}
pub fn fs_exists_sync(state: &Rc<RefCell<HostState>>, args: &[Value]) -> Result<Value, VmError> {
    crate::modules::fs::exists_sync(state, args)
}
pub fn fs_realpath_sync(state: &Rc<RefCell<HostState>>, args: &[Value]) -> Result<Value, VmError> {
    crate::modules::fs::realpath_sync(state, args)
}

// ---- net ----
pub fn net_connect(state: &Rc<RefCell<HostState>>, args: &[Value]) -> Result<Value, VmError> {
    crate::modules::net::connect(state, args)
}
pub fn net_is_ip(_: &Rc<RefCell<HostState>>, args: &[Value]) -> Result<Value, VmError> {
    Ok(Value::Number(crate::modules::net::is_ip(args) as i32 as f64))
}
pub fn net_is_ipv4(_: &Rc<RefCell<HostState>>, args: &[Value]) -> Result<Value, VmError> {
    Ok(Value::Boolean(crate::modules::net::is_ipv4(args)))
}
pub fn net_is_ipv6(_: &Rc<RefCell<HostState>>, args: &[Value]) -> Result<Value, VmError> {
    Ok(Value::Boolean(crate::modules::net::is_ipv6(args)))
}
pub fn net_create_server(state: &Rc<RefCell<HostState>>, args: &[Value]) -> Result<Value, VmError> {
    crate::modules::net::create_server(state, args)
}

// ---- http ----
pub fn http_request(state: &Rc<RefCell<HostState>>, args: &[Value]) -> Result<Value, VmError> {
    crate::modules::http::request(state, args)
}
pub fn http_get(state: &Rc<RefCell<HostState>>, args: &[Value]) -> Result<Value, VmError> {
    crate::modules::http::get(state, args)
}
pub fn http_create_server(
    state: &Rc<RefCell<HostState>>,
    args: &[Value],
) -> Result<Value, VmError> {
    crate::modules::http::create_server(state, args)
}

// ---- stream ----
pub fn stream_pipeline(state: &Rc<RefCell<HostState>>, args: &[Value]) -> Result<Value, VmError> {
    crate::modules::stream::pipeline(state, args)
}
pub fn stream_readable(state: &Rc<RefCell<HostState>>, args: &[Value]) -> Result<Value, VmError> {
    crate::modules::stream::new_readable(state, args)
}
pub fn stream_writable(state: &Rc<RefCell<HostState>>, args: &[Value]) -> Result<Value, VmError> {
    crate::modules::stream::new_writable(state, args)
}
pub fn stream_duplex(state: &Rc<RefCell<HostState>>, args: &[Value]) -> Result<Value, VmError> {
    crate::modules::stream::new_duplex(state, args)
}
pub fn stream_transform(state: &Rc<RefCell<HostState>>, args: &[Value]) -> Result<Value, VmError> {
    crate::modules::stream::new_transform(state, args)
}

// ---- string_decoder ----
pub fn string_decoder_new(
    state: &Rc<RefCell<HostState>>,
    args: &[Value],
) -> Result<Value, VmError> {
    crate::modules::string_decoder::new_decoder(state, args)
}

// ---- require ----
pub fn node_require(state: &Rc<RefCell<HostState>>, args: &[Value]) -> Result<Value, VmError> {
    crate::modules::require::require(state, args)
}

// ---- readline ----
pub fn readline_create_interface(
    _: &Rc<RefCell<HostState>>,
    _: &[Value],
) -> Result<Value, VmError> {
    Ok(Value::Undefined)
}
