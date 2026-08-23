use crate::host::HostState;
use quench_runtime::value::Value;
use std::cell::RefCell;
use std::rc::Rc;

pub(crate) fn resolve(state: &Rc<RefCell<HostState>>, spec: &str) -> Option<Value> {
    let name = spec.strip_prefix("node:").unwrap_or(spec);
    if let Some(v) = resolve_basic_modules(state, name) { return Some(v); }
    if let Some(v) = resolve_io_modules(state, name) { return Some(v); }
    resolve_compat_modules(state, name)
}

fn resolve_basic_modules(
    state: &Rc<RefCell<HostState>>,
    name: &str,
) -> Option<Value> {
    match name {
        "console" => Some(crate::modules::console::build_value()),
        "process" => Some(crate::modules::process::build(
            &state.borrow().process.argv,
            &state.borrow().process.exec_path,
        )),
        "crypto" => Some(crate::modules::crypto::build()),
        "buffer" => Some(crate::modules::buffer::build_module()),
        "util" => Some(crate::host::namespace_object_from_pairs(
            crate::modules::util::build(),
        )),
        "internal/util" => Some(crate::host::namespace_object_from_pairs(vec![(
            "sleep".to_string(),
            crate::host::capability(crate::registry::SPEC_INTERNAL_UTIL_SLEEP),
        )])),
        "internal/event_target" => Some(crate::host::namespace_object_from_pairs(vec![(
            "kWeakHandler".to_string(),
            Value::String("kWeakHandler\0quench".to_string()),
        )])),
        "path" => Some(crate::modules::path::build()),
        "url" => Some(crate::modules::url::build_root(state)),
        "querystring" => Some(crate::modules::querystring::build()),
        "events" => Some(crate::modules::events::build()),
        "string_decoder" => Some(crate::host::namespace_object_from_pairs(
            crate::modules::string_decoder::build(),
        )),
        _ => None,
    }
}

fn resolve_io_modules(
    state: &Rc<RefCell<HostState>>,
    name: &str,
) -> Option<Value> {
    if let Some(v) = resolve_io_basic(state, name) { return Some(v); }
    resolve_io_cached(state, name)
}

fn resolve_io_basic(
    state: &Rc<RefCell<HostState>>,
    name: &str,
) -> Option<Value> {
    match name {
        "os" => Some(crate::host::namespace_object_from_pairs(crate::modules::os::build())),
        "dns" => Some(crate::modules::dns::build()),
        "net" => Some(crate::modules::net::build()),
        "tty" => Some(crate::modules::tty::build()),
        "fs" => Some(crate::modules::fs::build()),
        "http" => Some(crate::modules::http::build()),
        "readline" => Some(crate::modules::readline::build()),
        "vm" => Some(crate::modules::vm::build()),
        "zlib" => Some(crate::modules::zlib::build()),
        "timers" | "timers/promises" => Some(crate::host::namespace_object_from_pairs(
            crate::modules::timers::build(),
        )),
        "tls" => Some(crate::host::namespace_object_from_pairs(vec![])),
        _ => None,
    }
}

fn resolve_io_cached(
    state: &Rc<RefCell<HostState>>,
    name: &str,
) -> Option<Value> {
    match name {
        "test" => state.borrow().node_test_module.clone()
            .or_else(|| crate::modules::node_test::build(state).ok()),
        "punycode" => state.borrow().punycode_module.clone()
            .or_else(|| crate::modules::punycode::build(state).ok()),
        "trace_events" => state.borrow().trace_events_module.clone()
            .or_else(|| crate::modules::trace_events::build(state).ok()),
        "perf_hooks" => state.borrow().perf_hooks_module.clone()
            .or_else(|| crate::modules::perf_hooks::build(state).ok()),
        _ => None,
    }
}

fn resolve_compat_modules(
    state: &Rc<RefCell<HostState>>,
    name: &str,
) -> Option<Value> {
    match name {
        "inspector" | "inspector/promises" => crate::modules::compat_extra::inspector(state).ok(),
        "repl" | "node:repl" => crate::modules::compat_extra::repl(state).ok(),
        "wasi" => crate::modules::compat_extra::wasi(state).ok(),
        "cluster" | "node:cluster" => crate::modules::compat_extra::cluster(state).ok(),
        "diagnostics_channel" | "node:diagnostics_channel" => {
            crate::modules::compat_extra::diagnostics_channel(state).ok()
        }
        "domain" | "node:domain" => crate::modules::compat_extra::domain(state).ok(),
        "v8" => crate::modules::compat_extra::v8(state).ok(),
        "worker_threads" => crate::modules::compat_extra::worker_threads(state).ok(),
        _ => None,
    }
}
