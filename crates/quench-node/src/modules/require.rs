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

pub fn require(state: &Rc<RefCell<HostState>>, args: &[Value]) -> Result<Value, VmError> {
    let spec = args.first().map(value_to_string).unwrap_or_default();
    // `node:assert` exports a callable value whose identity is stable
    // across requires (assert.strict === assert); cache it like a CJS module.
    if matches!(
        spec.as_str(),
        "assert" | "node:assert" | "assert/strict" | "node:assert/strict"
    ) {
        let key = "node:assert".to_string();
        if let Some(cached) = state.borrow().module_cache.get(&key) {
            return Ok(cached.clone());
        }
        let value = crate::modules::assert::build_value();
        state.borrow_mut().module_cache.insert(key, value.clone());
        return Ok(value);
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
        if let Some(cached) = state.borrow().module_cache.get("stream") {
            return Ok(cached.clone());
        }
        let value = crate::modules::stream::build(state)?;
        state
            .borrow_mut()
            .module_cache
            .insert("stream".to_string(), value.clone());
        return Ok(value);
    }
    if matches!(spec.as_str(), "async_hooks" | "node:async_hooks") {
        if let Some(cached) = state.borrow().module_cache.get("async_hooks") {
            return Ok(cached.clone());
        }
        let value = crate::modules::async_hooks::build(state)?;
        state
            .borrow_mut()
            .module_cache
            .insert("async_hooks".to_string(), value.clone());
    }
    if matches!(spec.as_str(), "punycode" | "node:punycode") {
        if let Some(cached) = state.borrow().module_cache.get("punycode") {
            return Ok(cached.clone());
        }
        let value = crate::modules::punycode::build(state)?;
        state
            .borrow_mut()
            .module_cache
            .insert("punycode".to_string(), value.clone());
        return Ok(value);
    }
    if matches!(spec.as_str(), "http-errors" | "node:http-errors") {
        if let Some(cached) = state.borrow().module_cache.get("http-errors") {
            return Ok(cached.clone());
        }
        let value = crate::modules::http_errors::build(state)?;
        state
            .borrow_mut()
            .module_cache
            .insert("http-errors".to_string(), value.clone());
    }
    if matches!(spec.as_str(), "statuses") {
        if let Some(cached) = state.borrow().module_cache.get("statuses") {
            return Ok(cached.clone());
        }
        let value = crate::modules::statuses::build(state)?;
        state
            .borrow_mut()
            .module_cache
            .insert("statuses".to_string(), value.clone());
        return Ok(value);
    }
    if matches!(spec.as_str(), "mime-db") {
        if let Some(cached) = state.borrow().module_cache.get("mime-db") {
            return Ok(cached.clone());
        }
        let value = crate::modules::mime_db::build(state)?;
        state.borrow_mut().module_cache.insert("mime-db".to_string(), value.clone());
        return Ok(value);
    }
    if matches!(spec.as_str(), "express" | "node:express") {
        if let Some(cached) = state.borrow().module_cache.get("express") {
            return Ok(cached.clone());
        }
        let value = crate::modules::express::build(state)?;
        state
            .borrow_mut()
            .module_cache
            .insert("express".to_string(), value.clone());
        return Ok(value);
    }
    if matches!(spec.as_str(), "express/lib/request") || spec == "node:express/request" {
        if let Some(cached) = state.borrow().module_cache.get("express_request") {
            return Ok(cached.clone());
        }
        let value = crate::modules::express_request::build(state)?;
        state
            .borrow_mut()
            .module_cache
            .insert("express_request".to_string(), value.clone());
        return Ok(value);
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
pub(crate) fn load_file_module(state: &Rc<RefCell<HostState>>, spec: &str) -> Result<Value, VmError> {
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
    state.borrow_mut().pending_modules.push(crate::host::PendingModule {
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
    // Build a module-scoped `require` closure that captures this
    // module's dirname AND the require_for host capability as closure
    // parameters (no global lookup, no context-guard dependency). The
    // closure is a real JS function returned by reducing a tiny factory
    // source and calling it with the capability + dir.
    let require_factory_src = "(function(cap, d) { return function(s) { return cap(d, s); }; })";
    let factory_ops = quench_runtime::reduce::reduce_global_script_source(require_factory_src)
        .map_err(|errors| VmError::EvalError(errors.join("; ")))?;
    let context = quench_runtime::vm::current_context();
    let mut registers = Vec::new();
    let factory = quench_runtime::vm::with_current_context(&context, || {
        quench_runtime::vm::execute_in_place_context(factory_ops.ops(), &mut registers, &context)
    })?;
    let require_for = crate::host::capability(crate::registry::SPEC_REQUIRE_FOR);
    let require_fn = quench_runtime::vm::call_value(
        &factory,
        &Value::Undefined,
        &[require_for, Value::String(pending.dirname.clone())],
    )?;
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

pub(crate) fn resolve(state: &Rc<RefCell<HostState>>, spec: &str) -> Option<Value> {
    let name = spec.strip_prefix("node:").unwrap_or(spec);
    match name {
        "console" => Some(crate::modules::console::build_value()),
        "process" => Some(crate::modules::process::build(
            &state.borrow().process.argv,
            &state.borrow().process.exec_path,
        )),
        "buffer" => Some(crate::modules::buffer::build_module()),
        "util" => Some(crate::host::namespace_object_from_pairs(
            crate::modules::util::build(),
        )),
        "internal/util" => Some(crate::host::namespace_object_from_pairs(vec![(
            "sleep".to_string(),
            crate::host::capability(crate::registry::SPEC_INTERNAL_UTIL_SLEEP),
        )])),
        // `internal/event_target` — only the public-test-facing symbol.
        "internal/event_target" => Some(crate::host::namespace_object_from_pairs(vec![(
            "kWeakHandler".to_string(),
            Value::String("kWeakHandler\0quench".to_string()),
        )])),
        "path" => Some(crate::modules::path::build()),
        "url" => Some(crate::modules::url::build_root(state)),
        "querystring" => Some(crate::modules::querystring::build()),
        "punycode" => {
            if let Some(cached) = state.borrow().punycode_module.clone() {
                Some(cached)
            } else {
                let value = crate::modules::punycode::build(state).ok()?;
                state.borrow_mut().punycode_module = Some(value.clone());
                Some(value)
            }
        }
        "os" => Some(crate::host::namespace_object_from_pairs(
            crate::modules::os::build(),
        )),
        "events" => Some(crate::modules::events::build()),
        "string_decoder" => Some(crate::host::namespace_object_from_pairs(
            crate::modules::string_decoder::build(),
        )),
        "dns" => Some(crate::modules::dns::build()),
        "net" => Some(crate::modules::net::build()),
        "tty" => Some(crate::modules::tty::build()),
        "fs" => Some(crate::modules::fs::build()),
        "http" => Some(crate::modules::http::build()),
        "readline" => Some(crate::modules::readline::build()),
        "vm" => Some(crate::modules::vm::build()),
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
        "zlib" => Some(crate::modules::zlib::build()),
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
        "worker_threads" => Some(crate::host::namespace_object_from_pairs(vec![
            ("isMainThread".to_string(), Value::Boolean(true)),
            (
                "Worker".to_string(),
                crate::host::capability(crate::registry::NodeSpec::new(
                    "worker_threads:Worker",
                    0x1900,
                )),
            ),
        ])),
        "sea" => Some(crate::host::namespace_object_from_pairs(vec![(
            "isSea".to_string(),
            crate::host::capability(crate::registry::NodeSpec::new("sea:isSea", 0x1a00)),
        )])),
        // `node:test` exports the callable `test` function itself, with
        // `test`, `describe`, and `it` aliases plus `.skip` variants.
        "test" => {
            use quench_runtime::execute::set_callable_property as attach;
            let test_fn = crate::host::capability(crate::registry::SPEC_TEST);
            let skip_fn = crate::host::capability(crate::registry::SPEC_TEST_SKIP);
            let _ = attach(&test_fn, "skip", skip_fn.clone());
            for alias in ["test", "describe", "it"] {
                let entry = crate::host::capability(crate::registry::SPEC_TEST);
                let _ = attach(&entry, "skip", skip_fn.clone());
                let _ = attach(&test_fn, alias, entry);
            }
            Some(test_fn)
        }
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
        "child_process" => Some(crate::host::namespace_object_from_pairs(vec![
            (
                "spawnSync".to_string(),
                crate::host::capability(crate::registry::SPEC_CP_SPAWNSYNC),
            ),
            (
                "execSync".to_string(),
                crate::host::capability(crate::registry::SPEC_CP_EXECSYNC),
            ),
            (
                "exec".to_string(),
                crate::host::capability(crate::registry::SPEC_CP_EXEC),
            ),
            (
                "spawn".to_string(),
                crate::host::capability(crate::registry::SPEC_CP_SPAWN),
            ),
        ])),
        _ => None,
    }
}

fn value_to_string(value: &Value) -> String {
    match value {
        Value::String(s) => s.clone(),
        _ => String::new(),
    }
}
