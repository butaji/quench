//! `require` global — the only host-side module resolver.
//!
//! Specifiers recognized:
//! - `node:http`, `node:events`, `node:buffer`, `node:util`,
//!   `node:path`, `node:url`, `node:querystring`, `node:os`,
//!   `node:process`, `node:console`, `node:stream`,
//!   `node:string_decoder`, `node:dns`, `node:net`, `node:tty`,
//!   `node:fs`, `node:timers`, `node:timers/promises`.
//!
//! Anything else throws `MODULE_NOT_FOUND`.

use std::cell::RefCell;
use std::rc::Rc;

use quench_runtime::execute::VmError;
use quench_runtime::host_api;
use quench_runtime::value::Value;

use crate::host::HostState;

pub fn require(_state: &Rc<RefCell<HostState>>, args: &[Value]) -> Result<Value, VmError> {
    let spec = args.first().map(value_to_string).unwrap_or_default();
    if let Some(ns) = resolve(&spec) {
        Ok(ns)
    } else {
        Err(VmError::EvalError(format!("Cannot find module '{spec}'")))
    }
}

fn resolve(spec: &str) -> Option<Value> {
    let name = spec.strip_prefix("node:").unwrap_or(spec);
    match name {
        "console" => Some(crate::modules::console::build_value()),
        "process" => Some(crate::modules::process::build()),
        "assert" => Some(crate::host::namespace_object_from_pairs(vec![
            ("ok".to_string(), crate::host::capability(crate::registry::NodeSpec::new("assert:ok", 0x1400))),
        ])),
        "buffer" => Some(crate::modules::buffer::build_module()),
        "util" => Some(crate::host::namespace_object_from_pairs(
            crate::modules::util::build(),
        )),
        "path" => Some(crate::host::namespace_object_from_pairs(
            crate::modules::path::build(),
        )),
        "url" => Some(crate::modules::url::build_root()),
        "querystring" => Some(crate::modules::querystring::build()),
        "os" => Some(crate::host::namespace_object_from_pairs(
            crate::modules::os::build(),
        )),
        "events" => Some(crate::modules::events::build()),
        "stream" => Some(crate::modules::stream::build()),
        "string_decoder" => Some(crate::host::namespace_object_from_pairs(
            crate::modules::string_decoder::build(),
        )),
        "dns" => Some(crate::modules::dns::build()),
        "net" => Some(crate::modules::net::build()),
        "tty" => Some(crate::modules::tty::build()),
        "fs" => Some(crate::modules::fs::build()),
        "http" => Some(crate::modules::http::build()),
        "readline" => Some(crate::modules::readline::build()),
        "vm" => Some(crate::host::namespace_object_from_pairs(vec![])),
        "dgram" => Some(crate::host::namespace_object_from_pairs(vec![
            ("createSocket".to_string(), crate::host::capability(crate::registry::NodeSpec::new("dgram:createSocket", 0x1500))),
        ])),
        "https" => Some(crate::host::namespace_object_from_pairs(vec![
            ("request".to_string(), crate::host::capability(crate::registry::NodeSpec::new("https:request", 0x1600))),
            ("get".to_string(), crate::host::capability(crate::registry::NodeSpec::new("https:get", 0x1601))),
        ])),
        "zlib" => Some(crate::host::namespace_object_from_pairs(vec![
            ("gzip".to_string(), crate::host::capability(crate::registry::NodeSpec::new("zlib:gzip", 0x1700))),
        ])),
        "perf_hooks" => Some(crate::host::namespace_object_from_pairs(vec![
            ("performance".to_string(), crate::host::capability(crate::registry::NodeSpec::new("perf_hooks:performance", 0x1800))),
        ])),
        "tls" => Some(crate::host::namespace_object_from_pairs(vec![])),
        "cluster" => Some(crate::host::namespace_object_from_pairs(vec![])),
        "inspector" => Some(crate::host::namespace_object_from_pairs(vec![])),
        "trace_events" => Some(crate::host::namespace_object_from_pairs(vec![])),
        "repl" => Some(crate::host::namespace_object_from_pairs(vec![])),
        "wasi" => Some(crate::host::namespace_object_from_pairs(vec![])),
        "worker_threads" => Some(crate::host::namespace_object_from_pairs(vec![
            ("Worker".to_string(), crate::host::capability(crate::registry::NodeSpec::new("worker_threads:Worker", 0x1900))),
        ])),
        "sea" => Some(crate::host::namespace_object_from_pairs(vec![
            ("isSea".to_string(), crate::host::capability(crate::registry::NodeSpec::new("sea:isSea", 0x1a00))),
        ])),
        "test" => Some(crate::host::namespace_object_from_pairs(vec![
            ("test".to_string(), crate::host::capability(crate::registry::NodeSpec::new("test:test", 0x1b00))),
        ])),
        "stream/web" => Some(crate::host::namespace_object_from_pairs(vec![
            ("ReadableStream".to_string(), crate::host::capability(crate::registry::NodeSpec::new("stream_web:ReadableStream", 0x1c00))),
        ])),
        "stream/consumers" => Some(crate::host::namespace_object_from_pairs(vec![
            ("text".to_string(), crate::host::capability(crate::registry::NodeSpec::new("stream_consumers:text", 0x1c01))),
        ])),
        "timers" => Some(crate::host::namespace_object_from_pairs(
            crate::modules::timers::build(),
        )),
        "timers/promises" => Some(crate::host::namespace_object_from_pairs(
            crate::modules::timers::build(),
        )),
        _ => None,
    }
}

fn value_to_string(value: &Value) -> String {
    match value {
        Value::String(s) => s.clone(),
        _ => String::new(),
    }
}
