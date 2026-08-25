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
    if matches!(spec.as_str(), "async_hooks" | "node:async_hooks") {
        let value = crate::modules::async_hooks::build();
        state.borrow_mut().module_cache.insert(spec, value.clone());
        return Ok(value);
    }
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
    if matches!(
        spec.as_str(),
        "timers" | "node:timers" | "timers/promises" | "node:timers/promises"
    ) {
        let timers_key = "node:timers";
        let promises_key = "node:timers/promises";
        let requested_key = if spec.ends_with("/promises") {
            promises_key
        } else {
            timers_key
        };
        if let Some(cached) = state.borrow().module_cache.get(requested_key) {
            return Ok(cached.clone());
        }
        let promises = crate::modules::timers::build_promises()
            .unwrap_or_else(|_| crate::host::namespace_object_from_pairs(Vec::new()));
        let mut bindings = crate::modules::timers::build();
        bindings.push(("promises".to_string(), promises.clone()));
        let timers = crate::host::namespace_object_from_pairs(bindings);
        state
            .borrow_mut()
            .module_cache
            .insert(timers_key.to_string(), timers.clone());
        state
            .borrow_mut()
            .module_cache
            .insert(promises_key.to_string(), promises);
        return Ok(if requested_key == promises_key {
            state
                .borrow()
                .module_cache
                .get(promises_key)
                .cloned()
                .unwrap_or(Value::Undefined)
        } else {
            timers
        });
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
    let wrapped = wrap_cjs(state, &filename, source);
    let module = state
        .borrow()
        .pending_module
        .as_ref()
        .map(|pending| pending.module.clone());
    let program = quench_runtime::reduce::reduce_global_script_source(&wrapped)
        .map_err(|errors| VmError::EvalError(errors.join("; ")))?;
    let context = quench_runtime::vm::current_context();
    // Re-entrant execution: `execute_with_context` would reset the
    // runtime's locals state and corrupt the frame that called `require`.
    let mut registers = quench_runtime::register_file::RegisterFile::new();
    quench_runtime::vm::execute_code_in_place_context(program.code(), &mut registers, &context)?;
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
    state.borrow_mut().pending_module = Some(crate::host::PendingModule {
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

fn resolve(state: &Rc<RefCell<HostState>>, spec: &str) -> Option<Value> {
    let name = spec.strip_prefix("node:").unwrap_or(spec);
    match name {
        "console" => Some(crate::modules::console::build_value()),
        "process" => Some(crate::modules::process::build(
            &state.borrow().process.argv,
            &state.borrow().process.exec_path,
        )),
        "buffer" => Some(crate::modules::buffer::build_module()),
        "internal/buffer" => Some(crate::host::namespace_object_from_pairs(vec![(
            "utf8Write".to_string(),
            crate::host::capability(crate::registry::SPEC_INTERNAL_BUFFER_UTF8_WRITE),
        )])),
        "util" => Some(crate::host::namespace_object_from_pairs(
            crate::modules::util::build(),
        )),
        "util/types" => {
            let util = require(state, &[Value::String("util".into())]).ok()?;
            Some(quench_runtime::execute::get_property(&util, "types"))
        }
        "internal/util" => Some(crate::host::namespace_object_from_pairs(vec![(
            "sleep".to_string(),
            crate::host::capability(crate::registry::SPEC_INTERNAL_UTIL_SLEEP),
        )])),
        "internal/test/binding" => Some(crate::host::namespace_object_from_pairs(vec![(
            "internalBinding".to_string(),
            crate::host::capability(crate::registry::SPEC_INTERNAL_BINDING),
        )])),
        "internal/linkedlist" => Some(crate::host::namespace_object_from_pairs(vec![
            (
                "init".to_string(),
                crate::host::capability(crate::registry::SPEC_LINKED_LIST_INIT),
            ),
            (
                "remove".to_string(),
                crate::host::capability(crate::registry::SPEC_LINKED_LIST_REMOVE),
            ),
            (
                "append".to_string(),
                crate::host::capability(crate::registry::SPEC_LINKED_LIST_APPEND),
            ),
            (
                "isEmpty".to_string(),
                crate::host::capability(crate::registry::SPEC_LINKED_LIST_IS_EMPTY),
            ),
            (
                "peek".to_string(),
                crate::host::capability(crate::registry::SPEC_LINKED_LIST_PEEK),
            ),
        ])),
        "internal/errors" => Some(crate::host::namespace_object_from_pairs(vec![(
            "codes".to_string(),
            crate::host::namespace_object_from_pairs(vec![
                (
                    "ERR_OUT_OF_RANGE".to_string(),
                    Value::Builtin(quench_runtime::ops::Builtin::RangeError),
                ),
                (
                    "ERR_IPC_CHANNEL_CLOSED".to_string(),
                    Value::Builtin(quench_runtime::ops::Builtin::Error),
                ),
            ]),
        )])),
        // `internal/event_target` — only the public-test-facing symbol.
        "internal/event_target" => {
            let custom_event = crate::host::capability(crate::registry::SPEC_CUSTOM_EVENT);
            let _ = quench_runtime::execute::set_callable_property(
                &custom_event,
                "prototype",
                crate::host::namespace_object_from_pairs(Vec::new()),
            );
            for (name, value) in [
                ("NONE", 0.0),
                ("CAPTURING_PHASE", 1.0),
                ("AT_TARGET", 2.0),
                ("BUBBLING_PHASE", 3.0),
            ] {
                let _ = quench_runtime::execute::set_callable_property(
                    &custom_event,
                    name,
                    Value::Number(value),
                );
            }
            let _ = quench_runtime::execute::set_callable_property(
                &custom_event,
                "length",
                Value::Number(1.0),
            );
            Some(crate::host::namespace_object_from_pairs(vec![
                (
                    "Event".to_string(),
                    crate::host::capability(crate::registry::SPEC_EVENT),
                ),
                ("CustomEvent".to_string(), custom_event),
                (
                    "defineEventHandler".to_string(),
                    crate::host::capability(crate::registry::SPEC_DEFINE_EVENT_HANDLER),
                ),
                (
                    "EventTarget".to_string(),
                    crate::host::capability(crate::registry::NodeSpec::new(
                        "events:EventTarget",
                        0x0116,
                    )),
                ),
                (
                    "NodeEventTarget".to_string(),
                    crate::host::capability(crate::registry::NodeSpec::new(
                        "events:EventTarget",
                        0x0116,
                    )),
                ),
                (
                    "kWeakHandler".to_string(),
                    Value::String("kWeakHandler\0quench".to_string()),
                ),
            ]))
        }
        "path" => Some(crate::modules::path::build()),
        "url" => Some(crate::modules::url::build_root(state)),
        "querystring" => Some(crate::modules::querystring::build()),
        "os" => Some(crate::host::namespace_object_from_pairs(
            crate::modules::os::build(),
        )),
        "events" => Some(crate::modules::events::build()),
        "diagnostics_channel" => crate::modules::compat_extra::diagnostics_channel(state).ok(),
        "domain" => crate::modules::compat_extra::domain(state).ok(),
        "async_hooks" => Some(crate::modules::async_hooks::build()),
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
        "worker_threads" => crate::modules::compat_extra::worker_threads(state).ok(),
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
            for alias in ["test", "describe", "it", "suite"] {
                let _ = attach(&test_fn, alias, test_fn.clone());
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
        "timers" => {
            let mut bindings = crate::modules::timers::build();
            let promises = crate::modules::timers::build_promises()
                .unwrap_or_else(|_| crate::host::namespace_object_from_pairs(Vec::new()));
            bindings.push(("promises".to_string(), promises));
            Some(crate::host::namespace_object_from_pairs(bindings))
        }
        "timers/promises" => Some(
            crate::modules::timers::build_promises()
                .unwrap_or_else(|_| crate::host::namespace_object_from_pairs(Vec::new())),
        ),
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
