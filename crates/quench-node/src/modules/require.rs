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

pub fn require(state: &Rc<RefCell<HostState>>, args: &[Value]) -> Result<Value, VmError> {
    let spec = args.first().map(value_to_string).unwrap_or_default();
    if let Some(ns) = resolve(&spec) {
        return Ok(ns);
    }
    load_file_module(state, &spec)
}

/// CommonJS file loader: resolve, cache, wrap, execute, return exports.
fn load_file_module(state: &Rc<RefCell<HostState>>, spec: &str) -> Result<Value, VmError> {
    let path = resolve_path(state, spec)?;
    let key = path.to_string_lossy().into_owned();
    if let Some(cached) = state.borrow().module_cache.get(&key) {
        return Ok(cached.clone());
    }
    let source = std::fs::read_to_string(&path)
        .map_err(|_| VmError::EvalError(format!("Cannot find module '{spec}'")))?;
    let exports = execute_module(state, &path, &source)?;
    state.borrow_mut().module_cache.insert(key, exports.clone());
    Ok(exports)
}

fn resolve_path(state: &Rc<RefCell<HostState>>, spec: &str) -> Result<std::path::PathBuf, VmError> {
    if !(spec.starts_with('.') || spec.starts_with('/')) {
        return Err(VmError::EvalError(format!("Cannot find module '{spec}'")));
    }
    let base = state
        .borrow()
        .dir_stack
        .last()
        .cloned()
        .unwrap_or_else(|| "/".to_string());
    let candidate = if spec.starts_with('/') {
        std::path::PathBuf::from(spec)
    } else {
        std::path::Path::new(&base).join(spec)
    };
    for path in [candidate.clone(), candidate.with_extension("js")] {
        if path.is_file() {
            return path.canonicalize().map_err(|_| not_found(spec));
        }
    }
    Err(not_found(spec))
}

fn not_found(spec: &str) -> VmError {
    VmError::EvalError(format!("Cannot find module '{spec}'"))
}

/// Execute one CJS file inside the standard wrapper and return `module.exports`.
/// The wrapper function is handed to the `__quench_cjs_wrap__` capability,
/// which invokes it with the prepared module record (see `cjs_wrap`).
fn execute_module(
    state: &Rc<RefCell<HostState>>,
    path: &std::path::Path,
    source: &str,
) -> Result<Value, VmError> {
    let exports = host_api::object(vec![]);
    let module = host_api::object(vec![("exports".to_string(), exports)]);
    let dirname = path
        .parent()
        .map(|p| p.to_string_lossy().into_owned())
        .unwrap_or_else(|| "/".to_string());
    state.borrow_mut().pending_module = Some(crate::host::PendingModule {
        module: module.clone(),
        filename: path.to_string_lossy().into_owned(),
        dirname,
    });
    let wrapped = format!("__quench_cjs_wrap__(function (exports, require, module, __filename, __dirname) {{\n{source}\n}})");
    let program = quench_runtime::reduce::reduce_source(&wrapped)
        .map_err(|errors| VmError::EvalError(errors.join("; ")))?;
    let context = quench_runtime::vm::current_context();
    quench_runtime::vm::execute_with_context(program.ops(), &context)?;
    quench_runtime::execute::get_property_result(&module, "exports")
}

/// `__quench_cjs_wrap__(fn)` — invoke a CJS wrapper with the pending record.
pub fn cjs_wrap(state: &Rc<RefCell<HostState>>, args: &[Value]) -> Result<Value, VmError> {
    let Some(function) = args.first() else {
        return Err(VmError::NotCallable);
    };
    let Some(pending) = state.borrow_mut().pending_module.take() else {
        return Err(VmError::EvalError("cjs wrap without pending module".into()));
    };
    let exports = quench_runtime::execute::get_property_result(&pending.module, "exports")?;
    state.borrow_mut().dir_stack.push(pending.dirname.clone());
    let result = quench_runtime::vm::call_value(
        function,
        &Value::Undefined,
        &[
            exports,
            crate::host::capability(crate::registry::SPEC_REQUIRE),
            pending.module,
            Value::String(pending.filename),
            Value::String(pending.dirname),
        ],
    );
    state.borrow_mut().dir_stack.pop();
    result
}

fn resolve(spec: &str) -> Option<Value> {
    let name = spec.strip_prefix("node:").unwrap_or(spec);
    match name {
        "console" => Some(crate::modules::console::build_value()),
        "process" => Some(crate::modules::process::build()),
        "assert" => Some(crate::host::namespace_object_from_pairs(vec![(
            "ok".to_string(),
            crate::host::capability(crate::registry::NodeSpec::new("assert:ok", 0x1400)),
        )])),
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
        "dgram" => Some(crate::host::namespace_object_from_pairs(vec![(
            "createSocket".to_string(),
            crate::host::capability(crate::registry::NodeSpec::new("dgram:createSocket", 0x1500)),
        )])),
        "https" => Some(crate::host::namespace_object_from_pairs(vec![
            (
                "request".to_string(),
                crate::host::capability(crate::registry::NodeSpec::new("https:request", 0x1600)),
            ),
            (
                "get".to_string(),
                crate::host::capability(crate::registry::NodeSpec::new("https:get", 0x1601)),
            ),
        ])),
        "zlib" => Some(crate::host::namespace_object_from_pairs(vec![(
            "gzip".to_string(),
            crate::host::capability(crate::registry::NodeSpec::new("zlib:gzip", 0x1700)),
        )])),
        "perf_hooks" => Some(crate::host::namespace_object_from_pairs(vec![(
            "performance".to_string(),
            crate::host::capability(crate::registry::NodeSpec::new(
                "perf_hooks:performance",
                0x1800,
            )),
        )])),
        "tls" => Some(crate::host::namespace_object_from_pairs(vec![])),
        "cluster" => Some(crate::host::namespace_object_from_pairs(vec![])),
        "inspector" => Some(crate::host::namespace_object_from_pairs(vec![])),
        "trace_events" => Some(crate::host::namespace_object_from_pairs(vec![])),
        "repl" => Some(crate::host::namespace_object_from_pairs(vec![])),
        "wasi" => Some(crate::host::namespace_object_from_pairs(vec![])),
        "worker_threads" => Some(crate::host::namespace_object_from_pairs(vec![(
            "Worker".to_string(),
            crate::host::capability(crate::registry::NodeSpec::new(
                "worker_threads:Worker",
                0x1900,
            )),
        )])),
        "sea" => Some(crate::host::namespace_object_from_pairs(vec![(
            "isSea".to_string(),
            crate::host::capability(crate::registry::NodeSpec::new("sea:isSea", 0x1a00)),
        )])),
        "test" => Some(crate::host::namespace_object_from_pairs(vec![(
            "test".to_string(),
            crate::host::capability(crate::registry::NodeSpec::new("test:test", 0x1b00)),
        )])),
        "stream/web" => Some(crate::host::namespace_object_from_pairs(vec![(
            "ReadableStream".to_string(),
            crate::host::capability(crate::registry::NodeSpec::new(
                "stream_web:ReadableStream",
                0x1c00,
            )),
        )])),
        "stream/consumers" => Some(crate::host::namespace_object_from_pairs(vec![(
            "text".to_string(),
            crate::host::capability(crate::registry::NodeSpec::new(
                "stream_consumers:text",
                0x1c01,
            )),
        )])),
        "stream/promises" => Some(crate::host::namespace_object_from_pairs(vec![
            (
                "finished".to_string(),
                crate::host::capability(crate::registry::NodeSpec::new(
                    "stream_promises:finished",
                    0x1c02,
                )),
            ),
            (
                "pipeline".to_string(),
                crate::host::capability(crate::registry::NodeSpec::new(
                    "stream_promises:pipeline",
                    0x1c03,
                )),
            ),
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
