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

/// Canonical internal/util namespace owned by the Rust host.
pub fn internal_util_module() -> Value {
    let enumerable = frozen_null_object(vec![(
        "enumerable".into(),
        Value::Boolean(true),
    )]);
    let empty = frozen_null_object(Vec::new());
    crate::host::namespace_object_from_pairs(vec![
        ("customInspectSymbol".into(), Value::String("Symbol.for.nodejs.util.inspect.custom\0".into())),
        ("pendingDeprecate".into(), crate::host::capability(crate::registry::SPEC_UTIL_DEPRECATE)),
        ("emitExperimentalWarning".into(), crate::host::capability(crate::registry::SPEC_INTERNAL_UTIL_EMIT_WARNING)),
        ("sleep".into(), crate::host::capability(crate::registry::SPEC_INTERNAL_UTIL_SLEEP)),
        ("assertCrypto".into(), crate::host::capability(crate::registry::SPEC_INTERNAL_UTIL_ASSERT_CRYPTO)),
        ("normalizeEncoding".into(), crate::host::capability(crate::registry::SPEC_INTERNAL_UTIL_NORMALIZE_ENCODING)),
        ("getCIDR".into(), crate::host::capability(crate::registry::SPEC_INTERNAL_UTIL_GET_CIDR)),
        ("constructSharedArrayBuffer".into(), crate::host::capability(crate::registry::SPEC_INTERNAL_UTIL_CONSTRUCT_SHARED_ARRAY_BUFFER)),
        ("customPromisifyArgs".into(), Value::String(crate::modules::util::PROMISIFY_CUSTOM_ARGS_KEY.into())),
        ("decorateErrorStack".into(), crate::host::capability(crate::registry::SPEC_INTERNAL_UTIL_DECORATE_ERROR_STACK)),
        ("assignFunctionName".into(), crate::host::capability(crate::registry::SPEC_INTERNAL_UTIL_ASSIGN_FUNCTION_NAME)),
        ("isError".into(), crate::host::capability(crate::registry::SPEC_INTERNAL_UTIL_IS_ERROR)),
        ("kEnumerableProperty".into(), enumerable),
        ("kEmptyObject".into(), empty),
    ])
}

fn frozen_null_object(properties: Vec<(String, Value)>) -> Value {
    let mut value = Value::object(properties);
    value = quench_runtime::execute::set_prototype_of(&value, &Value::Null).unwrap_or(value);
    for key in quench_runtime::execute::own_enumerable_keys(&value) {
        let current = quench_runtime::execute::get_property(&value, &key);
        let descriptor = host_api::object(vec![
            ("value".into(), current),
            ("writable".into(), Value::Boolean(false)),
            ("enumerable".into(), Value::Boolean(true)),
            ("configurable".into(), Value::Boolean(false)),
        ]);
        value = quench_runtime::execute::define_property(value, &key, descriptor)
            .unwrap_or_else(|_| Value::Undefined);
    }
    quench_runtime::execute::prevent_extensions(&value).unwrap_or(value)
}

fn placeholder_constructor(parent: Option<&Value>) -> Value {
    let prototype = host_api::object(Vec::new());
    let constructor = host_api::bound_builtin(
        quench_runtime::ops::Builtin::Object,
        Value::Undefined,
    );
    let constructor = quench_runtime::execute::set_property(constructor, "prototype", prototype);
    parent
        .and_then(|parent| quench_runtime::execute::set_prototype_of(&constructor, parent).ok())
        .unwrap_or(constructor)
}

fn global_constructor_or_capability(
    global: &Value,
    name: &str,
    spec: crate::registry::NodeSpec,
) -> Value {
    match quench_runtime::execute::get_property(global, name) {
        Value::Undefined => crate::host::capability(spec),
        value => value,
    }
}

pub fn require(state: &Rc<RefCell<HostState>>, args: &[Value]) -> Result<Value, VmError> {
    let spec = args.first().map(value_to_string).unwrap_or_default();
    let parent = state
        .borrow()
        .module_stack
        .last()
        .cloned()
        .unwrap_or_default();
    let event = crate::modules::diagnostics_channel::module_require_start(state, parent, spec.clone())?;
    let result = require_impl(state, args);
    if let Some(event) = event {
        crate::modules::diagnostics_channel::module_require_end(state, event, &result)?;
    }
    result
}

fn require_impl(state: &Rc<RefCell<HostState>>, args: &[Value]) -> Result<Value, VmError> {
    let spec = args.first().map(value_to_string).unwrap_or_default();
    // The Node test helpers are one host-owned resource.  Resolve every
    // spelling of the common entry point and tmpdir helper before filesystem
    // resolution; otherwise `common/index.js` loads a second JS copy whose
    // `./tmpdir` points at the checkout's `.tmp.0` directory.
    if spec == "../common"
        || spec == "../common/index"
        || spec.ends_with("/common")
        || spec.ends_with("/common/index")
    {
        let common = quench_runtime::execute::get_property(
            &quench_runtime::vm::current_global_object(),
            "__nodeCommon",
        );
        if matches!(common, Value::Object(_) | Value::ObjectAlias(_)) {
            return Ok(common);
        }
    }
    if spec == "../common/tmpdir"
        || spec == "../common/tmpdir.js"
        || spec.ends_with("/common/tmpdir")
        || spec.ends_with("/common/tmpdir.js")
        || (matches!(spec.as_str(), "./tmpdir" | "./tmpdir.js")
            && state.borrow().dir_stack.last().is_some_and(|dir| {
                std::path::Path::new(dir)
                    .file_name()
                    .is_some_and(|name| name == "common")
            }))
    {
        let tmpdir = quench_runtime::execute::get_property(
            &quench_runtime::vm::current_global_object(),
            "__nodeTmpdir",
        );
        if matches!(tmpdir, Value::Object(_) | Value::ObjectAlias(_)) {
            return Ok(tmpdir);
        }
    }
    if matches!(spec.as_str(), "child_process" | "node:child_process") {
        let global = quench_runtime::vm::current_global_object();
        let module = quench_runtime::execute::get_property(&global, "__nodeRequireChildProcess");
        if matches!(module, Value::Object(_) | Value::ObjectAlias(_)) {
            let constructor = quench_runtime::execute::get_property(&module, "ChildProcess");
            let prototype = quench_runtime::execute::get_property(&constructor, "prototype");
            if matches!(prototype, Value::Object(_) | Value::ObjectAlias(_)) {
                state.borrow_mut().child_process_prototype = Some(prototype);
            }
            state
                .borrow_mut()
                .module_cache
                .insert("child_process".into(), module.clone());
            return Ok(module);
        }
        let factory = quench_runtime::execute::get_property(&global, "__nodeChildProcessModule");
        if matches!(factory, Value::Function(_) | Value::BoundFunction(_)) {
            let module = quench_runtime::execute::call(&factory, &Value::Undefined, &[])?;
            state
                .borrow_mut()
                .module_cache
                .insert("child_process".into(), module.clone());
            return Ok(module);
        }
    }
    if matches!(spec.as_str(), "internal/child_process" | "internal/child_process.js" | "node:internal/child_process") {
        // Keep the channel key as a stable host-owned property name.  The
        // channel object itself lives on process, so all internal consumers
        // observe one identity without introducing a second IPC runtime.
        let public = state
            .borrow()
            .module_cache
            .get("child_process")
            .cloned()
            .or_else(|| resolve(state, "child_process"))
            .unwrap_or(Value::Undefined);
        let child_process = quench_runtime::execute::get_property(&public, "ChildProcess");
        let spawn_sync = crate::host::capability(crate::registry::SPEC_CP_SPAWNSYNC);
        let internal = host_api::object(vec![
            ("ChildProcess".into(), child_process),
            ("spawnSync".into(), spawn_sync.clone()),
            ("\0originalSpawnSync".into(), spawn_sync),
            (
                "kChannelHandle".into(),
                Value::String("Symbol.kChannelHandle\0".into()),
            ),
        ]);
        state
            .borrow_mut()
            .module_cache
            .insert("internal/child_process".into(), internal.clone());
        return Ok(internal);
    }
    if let Some(mock) = crate::modules::test::mocked_module(&spec) {
        if crate::modules::test::mock_module_cache(&spec) {
            let key = format!("\0mock:{spec}");
            if let Some(cached) = state.borrow().module_cache.get(&key) {
                return Ok(cached.clone());
            }
            state.borrow_mut().module_cache.insert(key, mock.clone());
        }
        return Ok(mock);
    }
    if spec == "node:url" {
        let cached = state.borrow().module_cache.get("url").cloned();
        if let Some(value) = cached {
            state.borrow_mut().module_cache.insert(spec, value.clone());
            return Ok(value);
        }
    }
    if matches!(spec.as_str(), "async_hooks" | "node:async_hooks") {
        let value = crate::modules::async_hooks::build();
        state.borrow_mut().module_cache.insert(spec, value.clone());
        return Ok(value);
    }
    if matches!(spec.as_str(), "stream/consumers" | "node:stream/consumers") {
        let module = crate::modules::stream::build_consumers(state)?;
        state
            .borrow_mut()
            .module_cache
            .insert("stream/consumers".into(), module.clone());
        return Ok(module);
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
    if matches!(spec.as_str(), "assert/strict" | "node:assert/strict") {
        let key = "node:assert/strict".to_string();
        if let Some(cached) = state.borrow().module_cache.get(&key) {
            return Ok(cached.clone());
        }
        let assert = require(state, &[Value::String("node:assert".into())])?;
        let value = quench_runtime::execute::get_property(&assert, "\0quench:strict-namespace");
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
        let timers = crate::modules::timers::build_with_promises()
            .unwrap_or_else(|_| crate::host::namespace_object_from_pairs(Vec::new()));
        let promises = quench_runtime::execute::get_property(&timers, "promises");
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
    // `node:` is a spelling of the same builtin, not a second module
    // instance. Canonicalize the cache key before resolving so constructors,
    // prototypes, and mutable module state retain identity across spellings.
    let cache_key = spec.strip_prefix("node:").unwrap_or(&spec).to_string();
    if let Some(cached) = state.borrow().module_cache.get(&cache_key) {
        return Ok(cached.clone());
    }
    if let Some(ns) = resolve(state, &spec) {
        state
            .borrow_mut()
            .module_cache
            .insert(cache_key, ns.clone());
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
        .map_err(|errors| VmError::EvalError(format!("{filename}: {}", errors.join("; "))))?;
    let context = quench_runtime::vm::current_context()
        .as_ref()
        .clone()
        .with_source_text(source.to_owned());
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
    let module = host_api::object(vec![
        ("exports".to_string(), exports),
        ("filename".to_string(), Value::String(filename.to_string())),
    ]);
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
    let filename = pending.filename.clone();
    state.borrow_mut().dir_stack.push(pending.dirname.clone());
    state.borrow_mut().module_stack.push(filename);
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
    state.borrow_mut().module_stack.pop();
    state.borrow_mut().dir_stack.pop();
    result
}

fn resolve(state: &Rc<RefCell<HostState>>, spec: &str) -> Option<Value> {
    let name = spec.strip_prefix("node:").unwrap_or(spec);
    match name {
        "console" => state
            .borrow()
            .console_module
            .clone()
            .or_else(|| Some(crate::modules::console::build_value())),
        "process" => {
            let process = state.borrow();
            Some(crate::modules::process::build_with_title(
                &process.process.argv,
                &process.process.exec_path,
                &process.process.title,
            ))
        }
        "buffer" => Some(crate::modules::buffer::build_module()),
        "internal/buffer" => Some(crate::host::namespace_object_from_pairs(vec![(
            "utf8Write".to_string(),
            crate::host::capability(crate::registry::SPEC_INTERNAL_BUFFER_UTF8_WRITE),
        )])),
        "util" => {
            if let Some(module) = state.borrow().util_module.clone() {
                return Some(module);
            }
            let module = crate::host::namespace_object_from_pairs(crate::modules::util::build());
            state.borrow_mut().util_module = Some(module.clone());
            Some(module)
        }
        "util/types" => {
            let util = require(state, &[Value::String("util".into())]).ok()?;
            Some(quench_runtime::execute::get_property(&util, "types"))
        }
        "internal/util" => Some(crate::host::namespace_object_from_pairs(vec![
            (
                // Node's internal modules share the registry symbol used by
                // util.inspect.custom.  Keep the symbol spelling canonical
                // so symbol-keyed hooks resolve to the same identity as
                // Symbol.for("nodejs.util.inspect.custom").
                "customInspectSymbol".to_string(),
                Value::String("Symbol.for.nodejs.util.inspect.custom\0".into()),
            ),
            (
                "pendingDeprecate".to_string(),
                crate::host::capability(crate::registry::SPEC_UTIL_DEPRECATE),
            ),
            (
                "emitExperimentalWarning".to_string(),
                crate::host::capability(crate::registry::SPEC_INTERNAL_UTIL_EMIT_WARNING),
            ),
            (
                "sleep".to_string(),
                crate::host::capability(crate::registry::SPEC_INTERNAL_UTIL_SLEEP),
            ),
            (
                "assertCrypto".to_string(),
                crate::host::capability(crate::registry::SPEC_INTERNAL_UTIL_ASSERT_CRYPTO),
            ),
            (
                "normalizeEncoding".to_string(),
                crate::host::capability(crate::registry::SPEC_INTERNAL_UTIL_NORMALIZE_ENCODING),
            ),
            (
                "getCIDR".to_string(),
                crate::host::capability(crate::registry::SPEC_INTERNAL_UTIL_GET_CIDR),
            ),
            (
                "constructSharedArrayBuffer".to_string(),
                crate::host::capability(
                    crate::registry::SPEC_INTERNAL_UTIL_CONSTRUCT_SHARED_ARRAY_BUFFER,
                ),
            ),
            (
                "customPromisifyArgs".to_string(),
                Value::String(crate::modules::util::PROMISIFY_CUSTOM_ARGS_KEY.into()),
            ),
            (
                "decorateErrorStack".to_string(),
                crate::host::capability(crate::registry::SPEC_INTERNAL_UTIL_DECORATE_ERROR_STACK),
            ),
            (
                "assignFunctionName".to_string(),
                crate::host::capability(crate::registry::SPEC_INTERNAL_UTIL_ASSIGN_FUNCTION_NAME),
            ),
            (
                "isError".to_string(),
                crate::host::capability(crate::registry::SPEC_INTERNAL_UTIL_IS_ERROR),
            ),
            (
                "kEnumerableProperty".to_string(),
                frozen_null_object(vec![("enumerable".into(), Value::Boolean(true))]),
            ),
            (
                "kEmptyObject".to_string(),
                frozen_null_object(Vec::new()),
            ),
        ])),
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
        "internal/errors" => Some(crate::host::namespace_object_from_pairs(vec![
            (
                "DNSException".to_string(),
                crate::host::capability(crate::registry::SPEC_DNS_EXCEPTION),
            ),
            (
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
            ),
        ])),
        // `internal/event_target` — only the public-test-facing symbol.
        "internal/event_target" => {
            let global = quench_runtime::vm::current_global_object();
            let event_target = global_constructor_or_capability(
                &global,
                "EventTarget",
                crate::registry::SPEC_EVENT_TARGET_NEW,
            );
            let event =
                global_constructor_or_capability(&global, "Event", crate::registry::SPEC_EVENT);
            for (name, value) in [
                ("NONE", 0.0),
                ("CAPTURING_PHASE", 1.0),
                ("AT_TARGET", 2.0),
                ("BUBBLING_PHASE", 3.0),
            ] {
                let _ = quench_runtime::execute::set_callable_property(
                    &event,
                    name,
                    Value::Number(value),
                );
            }
            let node_event_target = {
                let constructor =
                    crate::host::capability(crate::registry::SPEC_NODE_EVENT_TARGET_NEW);
                let prototype = crate::modules::event_target::node_prototype();
                let event_prototype = quench_runtime::execute::get_property(
                    &quench_runtime::execute::get_property(&global, "EventTarget"),
                    "prototype",
                );
                let prototype =
                    quench_runtime::execute::set_prototype_of(&prototype, &event_prototype)
                        .unwrap_or(prototype);
                crate::modules::event_target::set_node_prototype(prototype.clone());
                let _ = quench_runtime::execute::set_callable_property(
                    &constructor,
                    "prototype",
                    prototype,
                );
                let _ = quench_runtime::execute::set_callable_property(
                    &constructor,
                    "defaultMaxListeners",
                    Value::Number(10.0),
                );
                constructor
            };
            let custom_event = global_constructor_or_capability(
                &global,
                "CustomEvent",
                crate::registry::SPEC_CUSTOM_EVENT,
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
                ("Event".to_string(), event),
                ("CustomEvent".to_string(), custom_event),
                (
                    "defineEventHandler".to_string(),
                    crate::host::capability(crate::registry::SPEC_DEFINE_EVENT_HANDLER),
                ),
                ("EventTarget".to_string(), event_target),
                ("NodeEventTarget".to_string(), node_event_target),
                (
                    "kWeakHandler".to_string(),
                    Value::String("kWeakHandler\0quench".to_string()),
                ),
                (
                    "kEvents".to_string(),
                    Value::String("Symbol.for.quench.event_target.events\0".to_string()),
                ),
            ]))
        }
        "path" => Some(crate::modules::path::build()),
        "url" => Some(crate::modules::url::build_root(state)),
        "querystring" => Some(crate::modules::querystring::build()),
        "os" => {
            let object = crate::host::namespace_object_from_pairs(crate::modules::os::build());
            let descriptor = host_api::object(vec![
                (
                    "value".into(),
                    quench_runtime::execute::get_property(&object, "EOL"),
                ),
                ("writable".into(), Value::Boolean(false)),
                ("enumerable".into(), Value::Boolean(true)),
                ("configurable".into(), Value::Boolean(true)),
            ]);
            Some(
                quench_runtime::execute::define_property(object, "EOL", descriptor)
                    .unwrap_or(Value::Undefined),
            )
        }
        "events" => Some(crate::modules::events::build()),
        "diagnostics_channel" => Some(crate::modules::diagnostics_channel::build()),
        "domain" => Some(crate::modules::domain::build(state)),
        "async_hooks" => Some(crate::modules::async_hooks::build()),
        "string_decoder" => Some(crate::host::namespace_object_from_pairs(
            crate::modules::string_decoder::build(),
        )),
        "dns" | "node:dns" => {
            let global = quench_runtime::vm::current_global_object();
            let module = quench_runtime::execute::get_property(&global, "\0quench:dns_module");
            (!matches!(module, Value::Undefined)).then_some(module)
        }
        "net" => Some(crate::modules::net::build_with_state(Some(state))),
        "tty" => {
            let write_stream = crate::host::capability(crate::registry::SPEC_TTY_WRITE_STREAM);
            let read_stream = crate::host::capability(crate::registry::SPEC_TTY_READ_STREAM);
            Some(crate::host::namespace_object_from_pairs(vec![
                ("isatty".into(), crate::host::capability(crate::registry::SPEC_TTY_ISATTY)),
                ("ReadStream".into(), read_stream),
                ("WriteStream".into(), write_stream),
            ]))
        }
        "fs" => Some(crate::modules::fs::build()),
        "http" => Some(crate::modules::http::build(state)),
        "readline" => Some(crate::modules::readline::build()),
        "vm" => Some(crate::modules::vm::build()),
        "dgram" | "node:dgram" => {
            let global = quench_runtime::vm::current_global_object();
            let module = quench_runtime::execute::get_property(&global, "\0quench:dgram_module");
            if !matches!(module, Value::Undefined) {
                return Some(module);
            }
            None
        }
        "https" => {
            let http = crate::modules::http::build(state);
            let http_agent = quench_runtime::execute::get_property(&http, "Agent");
            let http_agent_prototype = quench_runtime::execute::get_property(&http_agent, "prototype");
            let agent = quench_runtime::execute::set_property(
                crate::host::capability(crate::registry::SPEC_HTTPS_AGENT),
                "prototype",
                http_agent_prototype,
            );
            let global_agent = quench_runtime::execute::get_property(&http, "globalAgent");
            Some(crate::host::namespace_object_from_pairs(vec![
                (
                    "request".to_string(),
                    crate::host::capability(crate::registry::SPEC_HTTPS_REQUEST),
                ),
                (
                    "get".to_string(),
                    crate::host::capability(crate::registry::SPEC_HTTPS_GET),
                ),
                ("Agent".to_string(), agent),
                ("globalAgent".to_string(), global_agent),
            ]))
        }
        "zlib" => Some(crate::modules::zlib::build()),
        "perf_hooks" => {
            let hooks = quench_runtime::execute::get_property(
                &quench_runtime::vm::current_global_object(),
                "__nodePerfHooks",
            );
            if !matches!(hooks, Value::Undefined) {
                Some(hooks)
            } else {
                Some(crate::host::namespace_object_from_pairs(vec![]))
            }
        }
        "tls" => {
            Some(crate::modules::tls::build(state))
        }
        "cluster" => {
            if let Some(module) = state.borrow().cluster.module() {
                return Some(module);
            }
            let global = quench_runtime::vm::current_global_object();
            let existing = quench_runtime::execute::get_property(&global, "__nodeCluster");
            if !matches!(existing, Value::Undefined) {
                Some(existing)
            } else {
                Some(crate::modules::cluster::build(state))
            }
        }
        "inspector" => Some(crate::modules::inspector::build()),
        "trace_events" => Some(crate::host::namespace_object_from_pairs(vec![])),
        "repl" => Some(crate::modules::repl::build()),
        "wasi" => Some(crate::modules::wasi::build()),
        "worker_threads" => crate::modules::compat_extra::worker_threads(state).ok(),
        "sea" => Some(crate::host::namespace_object_from_pairs(vec![(
            "isSea".to_string(),
            crate::host::capability(crate::registry::SPEC_SEA_IS_SEA),
        )])),
        // `node:test` exports the callable `test` function itself, with
        // `test`, `describe`, and `it` aliases plus `.skip` variants.
        "test" => {
            use quench_runtime::execute::set_callable_property as attach;
            let test_fn = crate::host::capability(crate::registry::SPEC_TEST);
            let skip_fn = crate::host::capability(crate::registry::SPEC_TEST_SKIP);
            let _ = attach(&test_fn, "skip", skip_fn.clone());
            for alias in ["test", "it"] {
                let _ = attach(&test_fn, alias, test_fn.clone());
            }
            let nested_fn = crate::host::capability(crate::registry::SPEC_TEST_NESTED);
            for alias in ["describe", "suite"] {
                let _ = attach(&test_fn, alias, nested_fn.clone());
            }
            let _ = attach(&test_fn, "run", test_fn.clone());
            let _ = attach(
                &test_fn,
                "getTestContext",
                crate::host::capability(crate::registry::SPEC_TEST_GET_CONTEXT),
            );
            let _ = attach(
                &test_fn,
                "before",
                crate::host::capability(crate::registry::SPEC_TEST_BEFORE_EACH),
            );
            let _ = attach(
                &test_fn,
                "after",
                crate::host::capability(crate::registry::SPEC_TEST_AFTER_EACH),
            );
            let mock = quench_runtime::host_api::object(vec![
                (
                    "fn".to_string(),
                    crate::host::capability(crate::registry::SPEC_TEST_MOCK_FN),
                ),
                (
                    "method".to_string(),
                    crate::host::capability(crate::registry::SPEC_TEST_MOCK_METHOD),
                ),
                (
                    "getter".to_string(),
                    crate::host::capability(crate::registry::SPEC_TEST_MOCK_GETTER),
                ),
                (
                    "setter".to_string(),
                    crate::host::capability(crate::registry::SPEC_TEST_MOCK_SETTER),
                ),
                (
                    "property".to_string(),
                    crate::host::capability(crate::registry::SPEC_TEST_MOCK_PROPERTY),
                ),
                (
                    "timers".to_string(),
                    quench_runtime::host_api::object(vec![
                        (
                            "enable".to_string(),
                            crate::host::capability(crate::registry::SPEC_TEST_MOCK_TIMERS_ENABLE),
                        ),
                        (
                            "tick".to_string(),
                            crate::host::capability(crate::registry::SPEC_TEST_MOCK_TIMERS_TICK),
                        ),
                        (
                            "setTime".to_string(),
                            crate::host::capability(crate::registry::SPEC_TEST_MOCK_TIMERS_SETTIME),
                        ),
                        (
                            "reset".to_string(),
                            crate::host::capability(crate::registry::SPEC_TEST_MOCK_TIMERS_RESET),
                        ),
                    ]),
                ),
            ]);
            let _ = attach(&test_fn, "mock", mock);
            let _ = quench_runtime::execute::set_property_in_place(
                &quench_runtime::execute::get_property(&test_fn, "mock"),
                "reset",
                crate::host::capability(crate::registry::SPEC_TEST_MOCK_RESET),
            );
            Some(test_fn)
        }
        "stream/web" => {
            let global = quench_runtime::vm::current_global_object();
            match quench_runtime::execute::get_property(&global, "__quenchWebStreams") {
                Value::Object(_) | Value::ObjectAlias(_) => Some(
                    quench_runtime::execute::get_property(&global, "__quenchWebStreams"),
                ),
                _ => Some(crate::host::namespace_object_from_pairs(vec![(
                    "ReadableStream".to_string(),
                    crate::host::capability(crate::registry::NodeSpec::new(
                        "stream_web:ReadableStream",
                        0x1c00,
                    )),
                )])),
            }
        }
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
        "timers" => Some(
            crate::modules::timers::build_with_promises()
                .unwrap_or_else(|_| crate::host::namespace_object_from_pairs(Vec::new())),
        ),
        "timers/promises" => Some(
            crate::modules::timers::build_promises()
                .unwrap_or_else(|_| crate::host::namespace_object_from_pairs(Vec::new())),
        ),
        "child_process" => {
            let exec_file = crate::host::capability(crate::registry::SPEC_CP_EXECFILE);
            let constructor = crate::host::capability(crate::registry::SPEC_CP_CONSTRUCTOR);
            let prototype = host_api::object(vec![
                (
                    "kill".to_string(),
                    crate::host::capability(crate::registry::SPEC_CP_KILL),
                ),
                (
                    "ref".to_string(),
                    crate::host::capability(crate::registry::SPEC_PROCESS_REF),
                ),
                (
                    "unref".to_string(),
                    crate::host::capability(crate::registry::SPEC_PROCESS_UNREF),
                ),
                (
                    "spawn".to_string(),
                    crate::host::capability(crate::registry::SPEC_CP_INSTANCE_SPAWN),
                ),
            ]);
            let constructor =
                quench_runtime::execute::set_property(constructor, "prototype", prototype.clone());
            state.borrow_mut().child_process_prototype = Some(prototype);
            Some(crate::host::namespace_object_from_pairs(vec![
                (
                    "fork".to_string(),
                    crate::host::capability(crate::registry::SPEC_CP_FORK),
                ),
                ("ChildProcess".to_string(), constructor),
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
                ("execFile".to_string(), exec_file.clone()),
                ("\0quench:child_process_execFile".to_string(), exec_file),
                (
                    "execFileSync".to_string(),
                    crate::host::capability(crate::registry::SPEC_CP_EXECSYNC),
                ),
                (
                    "spawn".to_string(),
                    crate::host::capability(crate::registry::SPEC_CP_SPAWN),
                ),
            ]))
        }
        _ => None,
    }
}

fn value_to_string(value: &Value) -> String {
    match value {
        Value::String(s) => s.clone(),
        _ => String::new(),
    }
}
