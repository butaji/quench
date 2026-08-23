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
use crate::host::HostState;
use quench_runtime::execute::VmError;
use quench_runtime::host_api;
use quench_runtime::value::Value;
use std::cell::RefCell;
use std::rc::Rc;
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
    state
        .borrow_mut()
        .module_cache
        .insert(key.into(), value.clone());
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
fn require_assert(state: &Rc<RefCell<HostState>>, spec: &str) -> Option<Result<Value, VmError>> {
    matches!(
        spec,
        "assert" | "node:assert" | "assert/strict" | "node:assert/strict"
    )
    .then(|| {
        cached_module(state, "node:assert", || {
            Ok(crate::modules::assert::build_value())
        })
    })
}
fn require_path_variant(
    state: &Rc<RefCell<HostState>>,
    spec: &str,
) -> Option<Result<Value, VmError>> {
    if !matches!(
        spec,
        "path/posix" | "path/win32" | "node:path/posix" | "node:path/win32"
    ) {
        return None;
    }
    Some((|| {
        if let Some(cached) = state.borrow().module_cache.get(spec) {
            return Ok(cached.clone());
        }
        let path_mod = require(state, &[Value::String("path".into())])?;
        let key = spec.rsplit('/').next().unwrap_or("posix");
        let ns = quench_runtime::execute::get_property(&path_mod, key);
        state
            .borrow_mut()
            .module_cache
            .insert(spec.to_string(), ns.clone());
        Ok(ns)
    })())
}
fn require_cached_group_a(
    state: &Rc<RefCell<HostState>>,
    spec: &str,
) -> Option<Result<Value, VmError>> {
    let (key, build): (&str, Box<dyn FnOnce() -> Result<Value, VmError> + '_>) = match spec {
        "async_hooks" | "node:async_hooks" | "internal/async_hooks" => (
            "async_hooks",
            Box::new(|| crate::modules::async_hooks::build(state)),
        ),
        "test" | "node:test" => ("test", Box::new(|| crate::modules::node_test::build(state))),
        "repl" | "node:repl" => (
            "repl",
            Box::new(|| crate::modules::compat_extra::repl(state)),
        ),
        "punycode" | "node:punycode" => (
            "punycode",
            Box::new(|| crate::modules::punycode::build(state)),
        ),
        "perf_hooks" | "node:perf_hooks" => (
            "perf_hooks",
            Box::new(|| crate::modules::perf_hooks::build(state)),
        ),
        "trace_events" | "trace-events" | "node:trace_events" | "node:trace-events" => (
            "trace_events",
            Box::new(|| crate::modules::trace_events::build(state)),
        ),
        "http-errors" | "node:http-errors" => (
            "http-errors",
            Box::new(|| crate::modules::http_errors::build(state)),
        ),
        _ => return None,
    };
    Some(cached_module(state, key, build))
}
fn require_cached_group_b(
    state: &Rc<RefCell<HostState>>,
    spec: &str,
) -> Option<Result<Value, VmError>> {
    let (key, build): (&str, Box<dyn FnOnce() -> Result<Value, VmError> + '_>) = match spec {
        "http2" | "node:http2" => (
            "node:http2",
            Box::new(|| Ok(crate::modules::http2::build())),
        ),
        "quic" | "node:quic" => ("node:quic", Box::new(|| Ok(crate::modules::quic::build()))),
        "statuses" => (
            "statuses",
            Box::new(|| crate::modules::statuses::build(state)),
        ),
        "mime-db" => (
            "mime-db",
            Box::new(|| crate::modules::mime_db::build(state)),
        ),
        "express" | "node:express" => (
            "express",
            Box::new(|| crate::modules::express::build(state)),
        ),
        "express/lib/request" | "node:express/request" => (
            "express_request",
            Box::new(|| crate::modules::express_request::build(state)),
        ),
        "koa" | "node:koa" => ("koa", Box::new(|| crate::modules::koa::build(state))),
        "fastify" | "node:fastify" => (
            "fastify",
            Box::new(|| crate::modules::fastify::build(state)),
        ),
        "hono" | "node:hono" => ("hono", Box::new(|| crate::modules::hono::build(state))),
        _ => return None,
    };
    Some(cached_module(state, key, build))
}
fn require_cached(state: &Rc<RefCell<HostState>>, spec: &str) -> Option<Result<Value, VmError>> {
    require_cached_group_a(state, spec).or_else(|| require_cached_group_b(state, spec))
}
fn require_special(_state: &Rc<RefCell<HostState>>, spec: &str) -> Option<Result<Value, VmError>> {
    match spec {
        "module" | "node:module" => {
            let names = [
                "assert",
                "buffer",
                "crypto",
                "events",
                "fs",
                "http",
                "https",
                "module",
                "net",
                "os",
                "path",
                "perf_hooks",
                "process",
                "querystring",
                "stream",
                "timers",
                "tls",
                "url",
                "util",
                "vm",
                "worker_threads",
                "zlib",
            ];
            Some(Ok(namespace_of_owned(vec![(
                "builtinModules".to_string(),
                quench_runtime::host_api::array(
                    names
                        .into_iter()
                        .map(|name| Value::String(name.into()))
                        .collect(),
                ),
            )])))
        }
        "readline/promises" | "node:readline/promises" => Some(Ok(namespace_of(vec![(
            "createInterface",
            capability_value(crate::registry::SPEC_READLINE),
        )]))),
        "internal/test/binding" => Some(Ok(namespace_of(vec![(
            "internalBinding",
            capability_value(crate::registry::NodeSpec::new("internalBinding", 2066)),
        )]))),
        "sqlite" | "node:sqlite" => Some(cached_module(_state, "node:sqlite", || {
            Ok(namespace_of_owned(vec![(
                "DatabaseSync".to_string(),
                capability_value(crate::registry::SPEC_SQLITE_DATABASE_SYNC),
            )]))
        })),
        _ => None,
    }
}
pub fn require(state: &Rc<RefCell<HostState>>, args: &[Value]) -> Result<Value, VmError> {
    let spec = args.first().map(value_to_string).unwrap_or_default();
    if let Some(result) = require_assert(state, &spec)
        .or_else(|| require_path_variant(state, &spec))
        .or_else(|| require_cached(state, &spec))
        .or_else(|| require_special(state, &spec))
    {
        return result;
    }
    // `node:` is an alias, not a distinct module identity. Keep the
    // canonical builtin name in the cache so `require("path")` and
    // `require("node:path")` share the same object.
    let cache_key = spec.strip_prefix("node:").unwrap_or(&spec);
    if let Some(cached) = state.borrow().module_cache.get(cache_key) {
        return Ok(cached.clone());
    }
    if let Some(ns) = resolve(state, &spec) {
        state
            .borrow_mut()
            .module_cache
            .insert(cache_key.to_string(), ns.clone());
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
        .map_err(|_| not_found(&format!("{spec} (base={base})")))
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
    // Host/global bindings may arrive through a BindingCell when the wrapper
    // is invoked from a reduced global script.  Resolve it before entering
    // the host-side call path; `call_value` deliberately expects a callable
    // target rather than a lexical cell.
    let function = match function {
        Value::BindingCell(cell) => cell.borrow().clone(),
        value => value.clone(),
    };
    let Some(pending) = state.borrow_mut().pending_modules.pop() else {
        return Err(VmError::EvalError("cjs wrap without pending module".into()));
    };
    let exports = quench_runtime::execute::get_property_result(&pending.module, "exports")?;
    state.borrow_mut().dir_stack.push(pending.dirname.clone());
    let require_fn = module_require(state, &pending.dirname)?;
    let result = quench_runtime::vm::call_value(
        &function,
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

fn module_require(_state: &Rc<RefCell<HostState>>, _dirname: &str) -> Result<Value, VmError> {
    // The CJS wrapper already scopes resolution with `dir_stack`.  Returning
    // the canonical host require capability avoids constructing a JS-bound
    // RequireFor wrapper in a context that may not have its host handle
    // registered (notably the zlib worker's reduced context).
    Ok(crate::host::capability(crate::registry::SPEC_REQUIRE))
}

/// Build a namespace object from a list of static capability specs.
fn capability_namespace_static(pairs: Vec<(&str, crate::registry::NodeSpec)>) -> Option<Value> {
    Some(crate::host::namespace_object_from_pairs(
        pairs
            .into_iter()
            .map(|(name, spec)| (name.to_string(), crate::host::capability(spec)))
            .collect(),
    ))
}

pub(crate) fn resolve(state: &Rc<RefCell<HostState>>, spec: &str) -> Option<Value> {
    resolve_dispatch(state, spec)
}

fn resolve_dispatch(state: &Rc<RefCell<HostState>>, spec: &str) -> Option<Value> {
    let name = spec.strip_prefix("node:").unwrap_or(spec);
    resolve_dispatch_core(state, name)
        .or_else(|| resolve_dispatch_network(state, name))
        .or_else(|| resolve_dispatch_compat_modules(state, name))
        .or_else(|| resolve_dispatch_compat_namespaces(name))
}

fn resolve_dispatch_core(state: &Rc<RefCell<HostState>>, name: &str) -> Option<Value> {
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
        "stream" => Some(crate::modules::stream::build(state).ok()?),
        "path" => Some(crate::modules::path::build()),
        "url" => Some(crate::modules::url::build_root(state)),
        "querystring" => Some(crate::modules::querystring::build()),
        "test" => resolve_test(state),
        "punycode" => resolve_punycode(state),
        "trace_events" => resolve_trace_events(state),
        "os" => Some(namespace_of_owned(crate::modules::os::build())),
        "events" => Some(crate::modules::events::build()),
        "string_decoder" => Some(namespace_of_owned(crate::modules::string_decoder::build())),
        _ => None,
    }
}

fn resolve_dispatch_network(state: &Rc<RefCell<HostState>>, name: &str) -> Option<Value> {
    match name {
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
            (
                "request",
                crate::registry::NodeSpec::new("https:request", 0x1600),
            ),
            ("get", crate::registry::NodeSpec::new("https:get", 0x1601)),
        ]),
        "zlib" => Some(crate::modules::zlib::build()),
        "perf_hooks" => resolve_perf_hooks(state),
        "tls" => Some(namespace_of_owned(vec![])),
        _ => None,
    }
}

fn resolve_dispatch_compat_modules(state: &Rc<RefCell<HostState>>, name: &str) -> Option<Value> {
    match name {
        "cluster" | "node:cluster" => crate::modules::compat_extra::cluster(state).ok(),
        "diagnostics_channel" | "node:diagnostics_channel" => {
            crate::modules::compat_extra::diagnostics_channel(state).ok()
        }
        "domain" | "node:domain" => Some(
            cached_module(state, "domain", || {
                crate::modules::compat_extra::domain(state)
            })
            .ok()?,
        ),
        "v8" => crate::modules::compat_extra::v8(state).ok(),
        "inspector" | "inspector/promises" => crate::modules::compat_extra::inspector(state).ok(),
        "repl" | "node:repl" => crate::modules::compat_extra::repl(state).ok(),
        "wasi" => crate::modules::compat_extra::wasi(state).ok(),
        "worker_threads" => crate::modules::compat_extra::worker_threads(state).ok(),
        _ => None,
    }
}

fn resolve_dispatch_compat_namespaces(name: &str) -> Option<Value> {
    match name {
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
            ("execFile", crate::registry::SPEC_CP_EXECFILE),
            ("execFileSync", crate::registry::SPEC_CP_EXECFILESYNC),
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
