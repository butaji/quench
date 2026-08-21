//! `require` global — the only host-side module resolver.
//!
//! Specifiers recognized:
//! - `node:http`, `node:events`, `node:buffer`, `node:util`,
//!   `node:path`, `node:url`, `node:querystring`, `node:os`,
//!   `node:process`, `node:console`, `node:stream`, `node:assert`,
//!   `node:assert/strict`,
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

/// Cache-or-build helper used by every per-module branch in `require`.
/// Returns the cached module value if present, otherwise builds,
/// inserts into `module_cache` under `key`, and returns it.
fn cached_module<F>(state: &Rc<RefCell<HostState>>, key: &str, build: F) -> Result<Value, VmError>
where
    F: FnOnce() -> Result<Value, VmError>,
{
    if let Some(cached) = state.borrow().module_cache.get(key) {
        return Ok(cached.clone());
    }
    let value = build()?;
    state.borrow_mut().module_cache.insert(key.into(), value.clone());
    Ok(value)
}

/// Build a namespace object from `(name, value)` pairs, returning
/// `Value::Undefined` on failure. Avoids the deeply nested
/// `Some(namespace_object(vec![...]))` paren-counting burden at the
/// require call sites.
fn namespace_of(pairs: Vec<(&str, Value)>) -> Value {
    crate::host::namespace_object(pairs).unwrap_or(Value::Undefined)
}

/// Build a namespace object from owned `(String, Value)` pairs.
fn namespace_of_owned(pairs: Vec<(String, Value)>) -> Value {
    crate::host::namespace_object_from_pairs(pairs)
}

/// A bare capability property descriptor used when wrapping a
/// single-capability spec as a namespace object.
fn capability_value(spec: crate::registry::NodeSpec) -> Value {
    crate::host::capability(spec)
}

/// Wrap a list of `(name, spec)` pairs as a namespace object whose
/// values are capability descriptors.
fn capability_namespace(pairs: Vec<(&str, crate::registry::NodeSpec)>) -> Value {
    let owned = pairs
        .into_iter()
        .map(|(name, spec)| (name.to_string(), capability_value(spec)))
        .collect();
    namespace_of_owned(owned)
}

pub fn require(state: &Rc<RefCell<HostState>>, args: &[Value]) -> Result<Value, VmError> {
    let spec = args.first().map(value_to_string).unwrap_or_default();
    // `node:assert` exports a callable value whose identity is stable
    // across requires (assert.strict === assert); cache it like a CJS module.
    if matches!(
        spec.as_str(),
        "assert" | "node:assert" | "assert/strict" | "node:assert/strict"
    ) {
        return cached_module(state, "node:assert", || {
            Ok(crate::modules::assert::build_value())
        });
    }
    if matches!(
        spec.as_str(),
        "path/posix" | "path/win32" | "node:path/posix" | "node:path/win32"
    ) {
        if let Some(cached) = state.borrow().module_cache.get(&spec) {
            return Ok(cached.clone());
        }
        let path_mod = require(state, &[Value::String("path".into())])?;
        let key = spec.rsplit('/').next().unwrap_or("posix");
        let ns = quench_runtime::execute::get_property(&path_mod, key);
        state.borrow_mut().module_cache.insert(spec, ns.clone());
        return Ok(ns);
    }
    if matches!(spec.as_str(), "stream" | "node:stream") {
        return cached_module(state, "stream", || crate::modules::stream::build(state));
    }
    if matches!(spec.as_str(), "async_hooks" | "node:async_hooks") {
        return cached_module(state, "async_hooks", || crate::modules::async_hooks::build(state));
    }
    if matches!(spec.as_str(), "test" | "node:test") {
        return cached_module(state, "test", || crate::modules::node_test::build(state));
    }
    if matches!(spec.as_str(), "readline/promises" | "node:readline/promises") {
        let value = namespace_of(vec![(
            "createInterface",
            capability_value(crate::registry::SPEC_READLINE),
        )]);
        return Ok(value);
    }
    if matches!(spec.as_str(), "punycode" | "node:punycode") {
        return cached_module(state, "punycode", || crate::modules::punycode::build(state));
    }
    if matches!(spec.as_str(), "perf_hooks" | "node:perf_hooks") {
        return cached_module(state, "perf_hooks", || crate::modules::perf_hooks::build(state));
    }
    if matches!(spec.as_str(), "trace_events" | "node:trace_events") {
        return cached_module(state, "trace_events", || crate::modules::trace_events::build(state));
    }
    if matches!(spec.as_str(), "http-errors" | "node:http-errors") {
        return cached_module(state, "http-errors", || crate::modules::http_errors::build(state));
    }
    if matches!(spec.as_str(), "sqlite" | "node:sqlite") {
        return cached_module(state, "node:sqlite", || {
            Ok(namespace_of_owned(vec![(
                "DatabaseSync".to_string(),
                capability_value(crate::registry::SPEC_SQLITE_DATABASE_SYNC),
            )]))
        });
    }
    if matches!(spec.as_str(), "http2" | "node:http2") {
        return cached_module(state, "node:http2", || Ok(crate::modules::http2::build()));
    }
    if matches!(spec.as_str(), "quic" | "node:quic") {
        return cached_module(state, "node:quic", || Ok(crate::modules::quic::build()));
    }
    if matches!(spec.as_str(), "statuses") {
        return cached_module(state, "statuses", || crate::modules::statuses::build(state));
    }
    if matches!(spec.as_str(), "mime-db") {
        return cached_module(state, "mime-db", || crate::modules::mime_db::build(state));
    }
    if matches!(spec.as_str(), "express" | "node:express") {
        return cached_module(state, "express", || crate::modules::express::build(state));
    }
    if matches!(spec.as_str(), "express/lib/request") || spec == "node:express/request" {
        return cached_module(state, "express_request", || {
            crate::modules::express_request::build(state)
        });
    }
    if matches!(spec.as_str(), "koa" | "node:koa") {
        return cached_module(state, "koa", || crate::modules::koa::build(state));
    }
    if matches!(spec.as_str(), "fastify" | "node:fastify") {
        return cached_module(state, "fastify", || crate::modules::fastify::build(state));
    }
    if let Some(cached) = state.borrow().module_cache.get(&spec) {
        return Ok(cached.clone());
    }
    if let Some(ns) = resolve(state, &spec) {
        state.borrow_mut().module_cache.insert(spec, ns.clone());
        return Ok(ns);
    }
    load_file_module(state, &spec)
}

/// CommonJS file loader: resolve, cache, wrap, execute, return exports.
pub(crate) fn load_file_module(
    state: &Rc<RefCell<HostState>>,
    spec: &str,
) -> Result<Value, VmError> {
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
    let base = state
        .borrow()
        .dir_stack
        .last()
        .cloned()
        .unwrap_or_else(|| "/".to_string());
    // oxc-resolver handles extension probing (.js), directory index files
    // (index.js), package.json mains, and the node_modules walk (with the
    // `exports`/`imports` maps and conditional exports) — the canonical
    // Node resolution algorithm. Relative and bare (npm package)
    // specifiers both resolve from the requiring module's directory.
    let resolver = oxc_resolver::Resolver::new(oxc_resolver::ResolveOptions {
        extensions: vec![".js".into()],
        main_files: vec!["index".into()],
        condition_names: vec!["node".into(), "require".into(), "default".into()],
        ..oxc_resolver::ResolveOptions::default()
    });
    resolver
        .resolve(std::path::Path::new(&base), spec)
        .map(|resolution| resolution.into_path_buf())
        .map_err(|_| not_found(spec))
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
    let filename = path.to_string_lossy().into_owned();
    quench_runtime::frame_stack::set_current_file(filename.clone());
    let wrapped = wrap_cjs(state, &filename, source);
    let module = state
        .borrow()
        .pending_modules
        .last()
        .map(|pending| pending.module.clone());
    let program = quench_runtime::reduce::reduce_global_script_source(&wrapped)
        .map_err(|errors| VmError::EvalError(errors.join("; ")))?;
    let context = quench_runtime::vm::current_context();
    // Re-entrant execution: `execute_with_context` would reset the
    // runtime's locals state and corrupt the frame that called `require`.
    let mut registers = Vec::new();
    quench_runtime::vm::execute_in_place_context(program.ops(), &mut registers, &context)?;
    let module = module.unwrap_or(Value::Undefined);
    quench_runtime::execute::get_property_result(&module, "exports")
}

/// Prepare `source` as a CJS module: records the pending module
/// record and returns the wrapped source. The caller reduces and
/// executes the result — in-place for nested `require`, in a fresh
/// frame for the main script (see `quench-node-test`'s runner).
pub fn wrap_cjs(state: &Rc<RefCell<HostState>>, filename: &str, source: &str) -> String {
    let exports = host_api::object(vec![]);
    let module = host_api::object(vec![("exports".to_string(), exports)]);
    let dirname = std::path::Path::new(filename)
        .parent()
        .map(|p| p.to_string_lossy().into_owned())
        .unwrap_or_else(|| "/".to_string());
    state
        .borrow_mut()
        .pending_modules
        .push(crate::host::PendingModule {
            module,
            filename: filename.to_string(),
            dirname,
        });
    format!("__quench_cjs_wrap__(function (exports, require, module, __filename, __dirname) {{\n{source}\n}})")
}

/// `__quench_cjs_wrap__(fn)` — invoke a CJS wrapper with the pending record.
pub fn cjs_wrap(state: &Rc<RefCell<HostState>>, args: &[Value]) -> Result<Value, VmError> {
    let Some(function) = args.first() else {
        return Err(VmError::NotCallable);
    };
    let Some(pending) = state.borrow_mut().pending_modules.pop() else {
        return Err(VmError::EvalError("cjs wrap without pending module".into()));
    };
    let exports = quench_runtime::execute::get_property_result(&pending.module, "exports")?;
    state.borrow_mut().dir_stack.push(pending.dirname.clone());
    let require_fn = module_require(state, &pending.dirname)?;
    let result = quench_runtime::vm::call_value(
        function,
        &Value::Undefined,
        &[
            exports,
            require_fn,
            pending.module,
            Value::String(pending.filename),
            Value::String(pending.dirname),
        ],
    );
    state.borrow_mut().dir_stack.pop();
    result
}

fn module_require(_state: &Rc<RefCell<HostState>>, dirname: &str) -> Result<Value, VmError> {
    let source = "(function(cap, d) { return function(s) { return cap(d, s); }; })";
    let ops = quench_runtime::reduce::reduce_global_script_source(source)
        .map_err(|errors| VmError::EvalError(errors.join("; ")))?;
    let context = quench_runtime::vm::current_context();
    let mut registers = Vec::new();
    let factory = quench_runtime::vm::with_current_context(&context, || {
        quench_runtime::vm::execute_in_place_context(ops.ops(), &mut registers, &context)
    })?;
    let require_for = crate::host::capability(crate::registry::SPEC_REQUIRE_FOR);
    quench_runtime::vm::call_value(
        &factory,
        &Value::Undefined,
        &[require_for, Value::String(dirname.to_string())],
    )
}

/// Build a namespace object from a list of static capability specs.
fn capability_namespace_static(
    pairs: Vec<(&str, crate::registry::NodeSpec)>,
) -> Option<Value> {
    Some(crate::host::namespace_object_from_pairs(
        pairs
            .into_iter()
            .map(|(name, spec)| (name.to_string(), crate::host::capability(spec)))
            .collect(),
    ))
}

pub(crate) fn resolve(state: &Rc<RefCell<HostState>>, spec: &str) -> Option<Value> {
    let name = spec.strip_prefix("node:").unwrap_or(spec);
    match name {
        "console" => Some(crate::modules::console::build_value()),
        "process" => Some(crate::modules::process::build(
            &state.borrow().process.argv,
            &state.borrow().process.exec_path,
        )),
        "crypto" => Some(crate::modules::crypto::build()),
        "buffer" => Some(crate::modules::buffer::build_module()),
        "util" => Some(namespace_of_owned(crate::modules::util::build())),
        "internal/util" => Some(namespace_of_owned(vec![(
            "sleep".to_string(),
            capability_value(crate::registry::SPEC_INTERNAL_UTIL_SLEEP),
        )])),
        "internal/event_target" => Some(namespace_of_owned(vec![(
            "kWeakHandler".to_string(),
            Value::String("kWeakHandler\0quench".to_string()),
        )])),
        "path" => Some(crate::modules::path::build()),
        "url" => Some(crate::modules::url::build_root(state)),
        "querystring" => Some(crate::modules::querystring::build()),
        "test" => resolve_test(state),
        "punycode" => resolve_punycode(state),
        "trace_events" => resolve_trace_events(state),
        "os" => Some(namespace_of_owned(crate::modules::os::build())),
        "events" => Some(crate::modules::events::build()),
        "string_decoder" => Some(namespace_of_owned(crate::modules::string_decoder::build())),
        "dns" => Some(crate::modules::dns::build()),
        "net" => Some(crate::modules::net::build()),
        "tty" => Some(crate::modules::tty::build()),
        "fs" => Some(crate::modules::fs::build()),
        "http" => Some(crate::modules::http::build()),
        "readline" => Some(crate::modules::readline::build()),
        "vm" => Some(crate::modules::vm::build()),
        "dgram" => capability_namespace_static(vec![(
            "createSocket",
            crate::registry::NodeSpec::new("dgram:createSocket", 0x2300),
        )]),
        "https" => capability_namespace_static(vec![
            ("request", crate::registry::NodeSpec::new("https:request", 0x1600)),
            ("get", crate::registry::NodeSpec::new("https:get", 0x1601)),
        ]),
        "zlib" => Some(crate::modules::zlib::build()),
        "perf_hooks" => resolve_perf_hooks(state),
        "tls" => Some(namespace_of_owned(vec![])),
        "cluster" => crate::modules::compat_extra::cluster(state).ok(),
        "domain" => crate::modules::compat_extra::domain(state).ok(),
        "v8" => crate::modules::compat_extra::v8(state).ok(),
        "inspector" => crate::modules::compat_extra::inspector(state).ok(),
        "repl" => crate::modules::compat_extra::repl(state).ok(),
        "wasi" => crate::modules::compat_extra::wasi(state).ok(),
        "worker_threads" => crate::modules::compat_extra::worker_threads(state).ok(),
        "sea" => capability_namespace_static(vec![(
            "isSea",
            crate::registry::NodeSpec::new("sea:isSea", 0x1a00),
        )]),
        "stream/web" => capability_namespace_static(vec![(
            "ReadableStream",
            crate::registry::NodeSpec::new("stream_web:ReadableStream", 0x1c00),
        )]),
        "stream/consumers" => capability_namespace_static(vec![(
            "text",
            crate::registry::NodeSpec::new("stream_consumers:text", 0x1c01),
        )]),
        "stream/promises" => capability_namespace_static(vec![
            (
                "finished",
                crate::registry::NodeSpec::new("stream_promises:finished", 0x1c02),
            ),
            (
                "pipeline",
                crate::registry::NodeSpec::new("stream_promises:pipeline", 0x1c03),
            ),
        ]),
        "timers" => Some(namespace_of_owned(crate::modules::timers::build())),
        "timers/promises" => Some(namespace_of_owned(crate::modules::timers::build())),
        "child_process" => capability_namespace_static(vec![
            ("spawnSync", crate::registry::SPEC_CP_SPAWNSYNC),
            ("execSync", crate::registry::SPEC_CP_EXECSYNC),
            ("exec", crate::registry::SPEC_CP_EXEC),
            ("spawn", crate::registry::SPEC_CP_SPAWN),
        ]),
        _ => None,
    }
}

fn resolve_test(state: &Rc<RefCell<HostState>>) -> Option<Value> {
    if let Some(cached) = state.borrow().node_test_module.clone() {
        Some(cached)
    } else {
        let value = crate::modules::node_test::build(state).ok()?;
        Some(value)
    }
}

fn resolve_punycode(state: &Rc<RefCell<HostState>>) -> Option<Value> {
    if let Some(cached) = state.borrow().punycode_module.clone() {
        Some(cached)
    } else {
        let value = crate::modules::punycode::build(state).ok()?;
        state.borrow_mut().punycode_module = Some(value.clone());
        Some(value)
    }
}

fn resolve_trace_events(state: &Rc<RefCell<HostState>>) -> Option<Value> {
    if let Some(cached) = state.borrow().trace_events_module.clone() {
        Some(cached)
    } else {
        let value = crate::modules::trace_events::build(state).ok()?;
        Some(value)
    }
}

fn resolve_perf_hooks(state: &Rc<RefCell<HostState>>) -> Option<Value> {
    if let Some(cached) = state.borrow().perf_hooks_module.clone() {
        Some(cached)
    } else {
        let value = crate::modules::perf_hooks::build(state).ok()?;
        state.borrow_mut().perf_hooks_module = Some(value.clone());
        Some(value)
    }
}

fn value_to_string(value: &Value) -> String {
    match value {
        Value::String(s) => s.clone(),
        _ => String::new(),
    }
}
