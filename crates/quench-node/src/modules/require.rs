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

use std::cell::Cell;
use std::cell::RefCell;
use std::rc::Rc;

use quench_runtime::execute::{self, VmError};
use quench_runtime::host_api;
use quench_runtime::value::Value;

use crate::host::HostState;

thread_local! {
    // Dynamic import is an ESM entry point even when the embedding host did
    // not expose the experimental CJS bridge flag.  Keep this as a scoped
    // loader fact rather than mutating observable process.execArgv.
    static DYNAMIC_ESM_LOAD: Cell<bool> = const { Cell::new(false) };
    static STATIC_ESM_LOAD: Cell<bool> = const { Cell::new(false) };
}

pub fn with_static_esm_mode<T>(enabled: bool, body: impl FnOnce() -> T) -> T {
    let previous = STATIC_ESM_LOAD.with(|flag| flag.replace(enabled));
    let result = body();
    STATIC_ESM_LOAD.with(|flag| flag.set(previous));
    result
}

pub fn require_dynamic(state: &Rc<RefCell<HostState>>, args: &[Value]) -> Result<Value, VmError> {
    let previous = DYNAMIC_ESM_LOAD.with(|flag| flag.replace(true));
    let result = (|| {
        let specifier = require_spec(args.first())?;
        if !specifier.starts_with("node:")
            && !std::path::Path::new(&specifier).extension().is_some()
        {
            let base = state
                .borrow()
                .dir_stack
                .last()
                .cloned()
                .unwrap_or_else(|| ".".to_string());
            let direct = std::path::Path::new(&base).join(&specifier);
            let package_dir = node_module_paths(std::path::Path::new(&base))
                .into_iter()
                .map(|root| root.join(&specifier))
                .find(|candidate| candidate.is_dir());
            if direct.is_dir() || (specifier.contains('/') && package_dir.is_some()) {
                return Err(invalid_value(format!(
                    "Directory import '{}' is not supported",
                    specifier
                )));
            }
        }
        require(state, args)
    })();
    DYNAMIC_ESM_LOAD.with(|flag| flag.set(previous));
    result
}

/// Materialize the namespace shape exposed by a dynamic import. CommonJS and
/// host modules contribute a `default` binding plus their enumerable exports;
/// ESM lowering already supplies named bindings on the same ordinary object.
pub fn dynamic_namespace(value: Value) -> Value {
    let mut properties = vec![("default".to_string(), value.clone())];
    if matches!(value, Value::Object(_) | Value::ObjectAlias(_)) {
        properties.extend(
            execute::own_enumerable_keys(&value)
                .into_iter()
                .filter(|key| key != "default")
                .map(|key| (key.clone(), execute::get_property(&value, &key))),
        );
    }
    host_api::object(properties)
}

pub fn dynamic_rejection(reason: Value) -> Value {
    let promise = Rc::new(quench_runtime::value::PromiseData::new(
        quench_runtime::value::PromiseState::Pending,
    ));
    let target = Rc::clone(&promise);
    quench_runtime::module_bindings::enqueue_job(Rc::new(move || {
        quench_runtime::module_bindings::reject_promise(&target, reason.clone());
    }));
    Value::Promise(promise)
}

pub fn dynamic_import_rejection(reason: Value) -> Value {
    if matches!(
        execute::get_property(&reason, "code"),
        Value::String(code) if code == "MODULE_NOT_FOUND"
    ) {
        let _ = execute::set_property_in_place(
            &reason,
            "code",
            Value::String("ERR_MODULE_NOT_FOUND".into()),
        );
    }
    dynamic_rejection(reason)
}

const BUILTIN_MODULES: &str = "assert assert/strict async_hooks buffer child_process cluster console crypto dgram diagnostics_channel dns domain events fs http http2 https module net os path path/posix path/win32 perf_hooks process punycode querystring readline repl stream stream/consumers string_decoder sys timers timers/promises tls tty url util util/types v8 vm worker_threads zlib inspector trace_events wasi node:test";
const INTERNAL_BUILTIN_MODULES: &str =
    "vfs sqlite _http_server internal/js_stream_socket internal/net";

/// Canonical internal/util namespace owned by the Rust host.
pub fn internal_util_module() -> Value {
    let enumerable = frozen_null_object(vec![("enumerable".into(), Value::Boolean(true))]);
    let empty = frozen_null_object(Vec::new());
    crate::host::namespace_object_from_pairs(vec![
        (
            "customInspectSymbol".into(),
            Value::String("Symbol.for.nodejs.util.inspect.custom\0".into()),
        ),
        (
            "pendingDeprecate".into(),
            crate::host::capability(crate::registry::SPEC_UTIL_DEPRECATE),
        ),
        (
            "emitExperimentalWarning".into(),
            crate::host::capability(crate::registry::SPEC_INTERNAL_UTIL_EMIT_WARNING),
        ),
        (
            "sleep".into(),
            crate::host::capability(crate::registry::SPEC_INTERNAL_UTIL_SLEEP),
        ),
        (
            "assertCrypto".into(),
            crate::host::capability(crate::registry::SPEC_INTERNAL_UTIL_ASSERT_CRYPTO),
        ),
        (
            "normalizeEncoding".into(),
            crate::host::capability(crate::registry::SPEC_INTERNAL_UTIL_NORMALIZE_ENCODING),
        ),
        (
            "getCIDR".into(),
            crate::host::capability(crate::registry::SPEC_INTERNAL_UTIL_GET_CIDR),
        ),
        (
            "constructSharedArrayBuffer".into(),
            crate::host::capability(
                crate::registry::SPEC_INTERNAL_UTIL_CONSTRUCT_SHARED_ARRAY_BUFFER,
            ),
        ),
        (
            "customPromisifyArgs".into(),
            Value::String(crate::modules::util::PROMISIFY_CUSTOM_ARGS_KEY.into()),
        ),
        (
            "decorateErrorStack".into(),
            crate::host::capability(crate::registry::SPEC_INTERNAL_UTIL_DECORATE_ERROR_STACK),
        ),
        (
            "assignFunctionName".into(),
            crate::host::capability(crate::registry::SPEC_INTERNAL_UTIL_ASSIGN_FUNCTION_NAME),
        ),
        (
            "isError".into(),
            crate::host::capability(crate::registry::SPEC_INTERNAL_UTIL_IS_ERROR),
        ),
        (
            "WeakReference".into(),
            crate::host::capability(crate::registry::SPEC_INTERNAL_UTIL_WEAK_REFERENCE_CONSTRUCT),
        ),
        ("kEnumerableProperty".into(), enumerable),
        ("kEmptyObject".into(), empty),
    ])
}

/// Canonical internal/errors namespace. Dynamic error codes are installed by
/// `E` into the shared `codes` object, so constructors and lookups retain one
/// identity across the module's consumers.
pub fn internal_errors_module() -> Value {
    let codes = crate::host::namespace_object_from_pairs(vec![
        (
            "ERR_OUT_OF_RANGE".into(),
            Value::Builtin(quench_runtime::ops::Builtin::RangeError),
        ),
        (
            "ERR_IPC_CHANNEL_CLOSED".into(),
            Value::Builtin(quench_runtime::ops::Builtin::Error),
        ),
    ]);
    let define = quench_runtime::host_api::bound_capability_with_arguments(
        quench_runtime::ops::HostCapabilityRef {
            realm: quench_runtime::ops::RealmId::ROOT,
            kind: quench_runtime::ops::HostCapabilityKind::Custom(
                crate::registry::SPEC_INTERNAL_ERRORS_E.cap,
            ),
        },
        vec![codes.clone()],
    );
    let system_error = crate::dispatch_handlers::internal_system_error_constructor();
    let abort_error = quench_runtime::host_api::bound_capability_with_arguments(
        quench_runtime::ops::HostCapabilityRef {
            realm: quench_runtime::ops::RealmId::ROOT,
            kind: quench_runtime::ops::HostCapabilityKind::Custom(
                crate::registry::SPEC_INTERNAL_ERRORS_CONSTRUCTOR.cap,
            ),
        },
        vec![
            Value::String("AbortError".into()),
            Value::Undefined,
            Value::Builtin(quench_runtime::ops::Builtin::Error),
        ],
    );
    let module = crate::host::namespace_object_from_pairs(vec![
        ("AbortError".into(), abort_error),
        (
            "DNSException".into(),
            crate::host::capability(crate::registry::SPEC_DNS_EXCEPTION),
        ),
        ("codes".into(), codes.clone()),
        ("E".into(), define),
        ("SystemError".into(), system_error),
        (
            "kIsNodeError".into(),
            Value::String("Symbol(kIsNodeError)\0quench".into()),
        ),
        (
            "hideStackFrames".into(),
            crate::host::capability(crate::registry::SPEC_INTERNAL_ERRORS_HIDE_STACK_FRAMES),
        ),
    ]);
    let access_denied = quench_runtime::host_api::bound_capability_with_arguments(
        quench_runtime::ops::HostCapabilityRef {
            realm: quench_runtime::ops::RealmId::ROOT,
            kind: quench_runtime::ops::HostCapabilityKind::Custom(
                crate::registry::SPEC_INTERNAL_ERRORS_CONSTRUCTOR.cap,
            ),
        },
        vec![
            Value::String("ERR_ACCESS_DENIED".into()),
            Value::String("Access denied: %s".into()),
            Value::Builtin(quench_runtime::ops::Builtin::Error),
        ],
    );
    let _ = quench_runtime::execute::set_property_in_place(
        &access_denied,
        "NoStackError",
        access_denied.clone(),
    );
    let _ =
        quench_runtime::execute::set_property_in_place(&codes, "ERR_ACCESS_DENIED", access_denied);
    module
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
    let constructor =
        host_api::bound_builtin(quench_runtime::ops::Builtin::Object, Value::Undefined);
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

fn eval_module_factory(source: &str) -> Option<Value> {
    let program = quench_runtime::reduce::reduce_global_script_source(source).ok()?;
    let context = quench_runtime::vm::current_context();
    let mut registers = quench_runtime::register_file::RegisterFile::new();
    quench_runtime::vm::execute_code_in_place_context(program.code(), &mut registers, &context).ok()
}

fn stream_promises_for(stream: Value) -> Value {
    let pipeline = crate::host::capability(crate::registry::SPEC_STREAM_PROMISES_PIPELINE);
    let finished = crate::host::capability(crate::registry::SPEC_STREAM_PROMISES_FINISHED);
    let promises = crate::host::namespace_object_from_pairs(vec![
        ("pipeline".into(), pipeline.clone()),
        ("finished".into(), finished.clone()),
    ]);
    let custom = crate::modules::util::PROMISIFY_CUSTOM_KEY;
    for (name, method) in [("pipeline", "pipeline"), ("finished", "finished")] {
        let original = execute::get_property(&stream, method);
        let promisified = execute::get_property(&promises, name);
        let _ = execute::set_property_in_place(&original, custom, promisified);
    }
    promises
}

fn stream_promises_module(state: &Rc<RefCell<HostState>>) -> Option<Value> {
    let stream = crate::modules::stream::build(state).ok()?;
    Some(stream_promises_for(stream))
}

fn internal_vfs_router_module() -> Option<Value> {
    let factory = eval_module_factory(
        "() => { const path = globalThis.__nodePath; return {\
          isUnderMountPoint(value, mountPoint) { const v = path.resolve(value), m = path.resolve(mountPoint); return m === path.parse(m).root || v === m || v.startsWith(m + path.sep); },\
          getRelativePath(value, mountPoint) { const r = path.relative(path.resolve(mountPoint), path.resolve(value)); return r ? '/' + r.split(path.sep).join('/') : '/'; },\
          isAbsolutePath: path.isAbsolute }; }",
    )?;
    execute::call(&factory, &Value::Undefined, &[]).ok()
}

fn internal_vfs_file_handle_module() -> Option<Value> {
    let factory = eval_module_factory(
        "() => { class VirtualFileHandle {\
          constructor(path, flags, mode) { this.path = path; this.flags = flags === undefined ? 'r' : flags; this.mode = mode === undefined ? 0o666 : mode; this.position = 0; this.closed = false; }\
          __check() { if (this.closed) { const e = new Error('file handle is closed'); e.code = 'EBADF'; throw e; } }\
          __stub() { this.__check(); const e = new Error('Method not implemented'); e.code = 'ERR_METHOD_NOT_IMPLEMENTED'; throw e; }\
          closeSync() { this.closed = true; } close() { this.closed = true; return Promise.resolve(); }\
          chmod() { return Promise.resolve(); } chown() { return Promise.resolve(); } utimes() { return Promise.resolve(); } datasync() { return Promise.resolve(); } sync() { return Promise.resolve(); }\
          readableWebStream() { return this.__stub(); } readLines() { return this.__stub(); } createReadStream() { return this.__stub(); } createWriteStream() { return this.__stub(); }\
          readSync() { return this.__stub(); } writeSync() { return this.__stub(); } readFileSync() { return this.__stub(); } writeFileSync() { return this.__stub(); } statSync() { return this.__stub(); } truncateSync() { return this.__stub(); }\
          read() { try { return Promise.reject(this.__stub()); } catch (e) { return Promise.reject(e); } } write() { try { return Promise.reject(this.__stub()); } catch (e) { return Promise.reject(e); } }\
          readFile() { try { return Promise.reject(this.__stub()); } catch (e) { return Promise.reject(e); } } writeFile() { try { return Promise.reject(this.__stub()); } catch (e) { return Promise.reject(e); } }\
          stat() { try { return Promise.reject(this.__stub()); } catch (e) { return Promise.reject(e); } } truncate() { try { return Promise.reject(this.__stub()); } catch (e) { return Promise.reject(e); } }\
          readv() { try { return Promise.reject(this.__stub()); } catch (e) { return Promise.reject(e); } } writev() { try { return Promise.reject(this.__stub()); } catch (e) { return Promise.reject(e); } } appendFile() { try { return Promise.reject(this.__stub()); } catch (e) { return Promise.reject(e); } }\
        } class MemoryFileHandle extends VirtualFileHandle { constructor(path, flags, mode, content, getStats) { super(path, flags, mode); this.content = content; this.getStats = getStats; } statSync() { if (typeof this.getStats !== 'function') { const e = new Error('File statistics are not available'); e.code = 'ERR_INVALID_STATE'; throw e; } return this.getStats(); } } Symbol.asyncDispose ||= Symbol('Symbol.asyncDispose'); VirtualFileHandle.prototype[Symbol.asyncDispose] = VirtualFileHandle.prototype.close; return { VirtualFileHandle, MemoryFileHandle }; }",
    )?;
    execute::call(&factory, &Value::Undefined, &[]).ok()
}

fn internal_vfs_fd_module() -> Option<Value> {
    let factory = eval_module_factory(
        "() => ({ getVirtualFd(fd) { return globalThis.__quenchVfsFdHandles?.get(fd); } })",
    )?;
    execute::call(&factory, &Value::Undefined, &[]).ok()
}

fn internal_vfs_memory_provider_module() -> Option<Value> {
    let factory =
        eval_module_factory("() => ({ MemoryProvider: globalThis.__nodeVfs?.MemoryProvider })")?;
    execute::call(&factory, &Value::Undefined, &[]).ok()
}

pub fn require(state: &Rc<RefCell<HostState>>, args: &[Value]) -> Result<Value, VmError> {
    let spec = args.first().map(value_to_string).unwrap_or_default();
    let parent = state
        .borrow()
        .module_stack
        .last()
        .cloned()
        .unwrap_or_default();
    let event =
        crate::modules::diagnostics_channel::module_require_start(state, parent, spec.clone())?;
    let result = require_impl(state, args);
    if let Some(event) = event {
        crate::modules::diagnostics_channel::module_require_end(state, event, &result)?;
    }
    result
}

/// Resolve a CommonJS specifier using the same resolver and directory stack
/// as `require`. Builtins retain Node's canonical bare spelling.
pub fn resolve_require(state: &Rc<RefCell<HostState>>, args: &[Value]) -> Result<Value, VmError> {
    let spec = require_spec(args.first())?;
    if is_builtin_name(&spec) || resolve(state, &spec).is_some() {
        return Ok(Value::String(spec.clone()));
    }
    if let Some(options) = args.get(1) {
        match execute::get_property(options, "paths") {
            Value::Undefined => {}
            Value::Array(paths) if paths.is_empty() => return Err(not_found(&spec)),
            Value::Array(paths) => return resolve_from_paths(state, &spec, &paths),
            _ => {
                return Err(invalid_value(
                    "The \"paths\" argument must be an array".into(),
                ))
            }
        }
    }
    let base = resolve_base(state, args.get(1))?;
    let path = resolve_path_from_base(state, &spec, &base)?;
    Ok(Value::String(path.to_string_lossy().into_owned()))
}

fn resolve_from_paths(
    state: &Rc<RefCell<HostState>>,
    spec: &str,
    paths: &quench_runtime::value::ArrayData,
) -> Result<Value, VmError> {
    for index in 0..paths.logical_len() {
        let Some(value) = paths.get(index) else {
            continue;
        };
        let path = match &value {
            Value::String(path) => path.clone(),
            Value::StringUnits(_) => value_to_string(&value),
            _ => return Err(invalid_paths_type(&value)),
        };
        let path = state.borrow().process.cwd.join(path);
        let base = if spec.starts_with('.') || std::path::Path::new(spec).is_absolute() {
            path
        } else if path.file_name().is_some_and(|name| name == "node_modules") {
            // `paths` entries are lookup roots.  A caller may pass an
            // already-materialized `node_modules` entry (as returned by
            // `require.resolve.paths`); peel that marker before the resolver
            // appends its own node_modules segment.
            let mut root = path;
            while root.file_name().is_some_and(|name| name == "node_modules") {
                let Some(parent) = root.parent() else { break };
                root = parent.to_path_buf();
            }
            root
        } else {
            path
        };
        let resolved = if !spec.starts_with('.') && !std::path::Path::new(spec).is_absolute() {
            resolve_bare_node_module(spec, &base)
        } else {
            resolve_path_from_base(state, spec, &base.to_string_lossy())
        };
        if let Ok(resolved) = resolved {
            return Ok(Value::String(resolved.to_string_lossy().into_owned()));
        }
    }
    Err(not_found(spec))
}

fn resolve_bare_node_module(
    spec: &str,
    base: &std::path::Path,
) -> Result<std::path::PathBuf, VmError> {
    let mut directory = base.to_path_buf();
    loop {
        let candidate = directory.join("node_modules").join(spec);
        if let Some(path) = probe_global_module_candidate(&candidate) {
            return Ok(path);
        }
        let Some(parent) = directory.parent() else {
            break;
        };
        if parent == directory {
            break;
        }
        directory = parent.to_path_buf();
    }
    Err(not_found(spec))
}

fn invalid_paths_type(value: &Value) -> VmError {
    let error = quench_runtime::builtins::error(
        quench_runtime::ops::Builtin::TypeError,
        &[Value::String(format!(
            "The \"paths\" argument must be an array of strings.{}",
            crate::modules::util::invalid_arg_received(value)
        ))],
    );
    VmError::Thrown(execute::set_property(
        error,
        "code",
        Value::String("ERR_INVALID_ARG_TYPE".into()),
    ))
}

pub fn resolve_capability() -> Value {
    host_api::capability_function_with_properties(
        crate::host::capability_ref(crate::registry::SPEC_REQUIRE_RESOLVE),
        vec![(
            "paths".into(),
            crate::host::capability(crate::registry::SPEC_REQUIRE_RESOLVE_PATHS),
        )],
    )
}

pub fn module_api(state: &Rc<RefCell<HostState>>) -> Value {
    if let Some(module) = state.borrow().module_api.clone() {
        return module;
    }
    let global = quench_runtime::vm::current_global_object();
    let global_require = execute::get_property(&global, "require");
    let cache = match execute::get_property(&global_require, "cache") {
        value @ (Value::Object(_) | Value::ObjectAlias(_)) => value,
        _ => host_api::object(Vec::new()),
    };
    let extensions = match execute::get_property(&global_require, "extensions") {
        value @ (Value::Object(_) | Value::ObjectAlias(_)) => value,
        _ => host_api::object(Vec::new()),
    };
    let module = host_api::object(vec![
        ("builtinModules".into(), builtin_modules_value()),
        ("_cache".into(), cache.clone()),
        ("_extensions".into(), extensions.clone()),
        (
            "createRequire".into(),
            crate::host::capability(crate::registry::SPEC_MODULE_CREATE_REQUIRE),
        ),
        (
            "isBuiltin".into(),
            crate::host::capability(crate::registry::SPEC_MODULE_IS_BUILTIN),
        ),
        (
            "_nodeModulePaths".into(),
            crate::host::capability(crate::registry::SPEC_MODULE_NODE_MODULE_PATHS),
        ),
        (
            "_resolveLookupPaths".into(),
            crate::host::capability(crate::registry::SPEC_MODULE_RESOLVE_LOOKUP_PATHS),
        ),
        (
            "_initPaths".into(),
            crate::host::capability(crate::registry::SPEC_MODULE_INIT_PATHS),
        ),
        (
            "_stat".into(),
            crate::host::capability(crate::registry::SPEC_MODULE_STAT),
        ),
        ("globalPaths".into(), host_api::array(Vec::new())),
        (
            "setSourceMapsSupport".into(),
            crate::host::capability(crate::registry::SPEC_MODULE_SET_SOURCEMAPS_SUPPORT),
        ),
        (
            "enableCompileCache".into(),
            crate::host::capability(crate::registry::SPEC_MODULE_ENABLE_COMPILE_CACHE),
        ),
        (
            "getCompileCacheDir".into(),
            crate::host::capability(crate::registry::SPEC_MODULE_GET_COMPILE_CACHE_DIR),
        ),
        (
            "flushCompileCache".into(),
            crate::host::capability(crate::registry::SPEC_MODULE_FLUSH_COMPILE_CACHE),
        ),
    ]);
    // Node exposes the constructor namespace as `module.Module`; preserve
    // identity so `Module.globalPaths` tracks the public array.
    let _ = execute::set_property_in_place(&module, "Module", module.clone());
    let _ = execute::set_property_in_place(&global_require, "cache", cache.clone());
    let _ = execute::set_property_in_place(&global_require, "extensions", extensions.clone());
    state.borrow_mut().module_extensions = Some(extensions.clone());
    state.borrow_mut().module_api = Some(module.clone());
    module
}

fn builtin_modules_value() -> Value {
    host_api::array(
        BUILTIN_MODULES
            .split_whitespace()
            .map(|name| Value::String(name.into()))
            .collect(),
    )
}

fn require_spec(value: Option<&Value>) -> Result<String, VmError> {
    match value {
        Some(Value::String(spec)) => Ok(spec.clone()),
        Some(Value::StringUnits(_)) => Ok(value_to_string(value.unwrap())),
        Some(value) => Err(VmError::Thrown(host_api::object(vec![
            ("name".into(), Value::String("TypeError".into())),
            ("code".into(), Value::String("ERR_INVALID_ARG_TYPE".into())),
            (
                "message".into(),
                Value::String(format!(
                    "The \"request\" argument must be of type string.{}",
                    crate::modules::util::invalid_arg_received(value)
                )),
            ),
        ]))),
        None => Err(VmError::Thrown(host_api::object(vec![
            ("name".into(), Value::String("TypeError".into())),
            ("code".into(), Value::String("ERR_INVALID_ARG_TYPE".into())),
            (
                "message".into(),
                Value::String("The \"request\" argument must be of type string".into()),
            ),
        ]))),
    }
}

fn is_builtin_name(spec: &str) -> bool {
    if spec == "node:test" {
        return true;
    }
    let name = spec.strip_prefix("node:").unwrap_or(spec);
    BUILTIN_MODULES
        .split_whitespace()
        .chain(INTERNAL_BUILTIN_MODULES.split_whitespace())
        .any(|candidate| candidate == name)
}

pub fn resolve_require_paths(
    state: &Rc<RefCell<HostState>>,
    args: &[Value],
) -> Result<Value, VmError> {
    let spec = require_spec(args.first())?;
    if is_builtin_name(&spec) || resolve(state, &spec).is_some() {
        return Ok(Value::Null);
    }
    let base = state
        .borrow()
        .dir_stack
        .last()
        .cloned()
        .unwrap_or_else(|| ".".into());
    if spec.starts_with('.') || std::path::Path::new(&spec).is_absolute() {
        return Ok(host_api::array(vec![Value::String(base)]));
    }
    Ok(host_api::array(
        node_module_paths(std::path::Path::new(&base))
            .into_iter()
            .map(|path| Value::String(path.to_string_lossy().into_owned()))
            .collect(),
    ))
}

pub fn module_is_builtin(args: &[Value]) -> Result<Value, VmError> {
    Ok(Value::Boolean(
        args.first()
            .and_then(|value| {
                matches!(value, Value::String(_) | Value::StringUnits(_))
                    .then(|| value_to_string(value))
            })
            .is_some_and(|name| is_builtin_name(&name)),
    ))
}

pub fn module_node_module_paths(args: &[Value]) -> Result<Value, VmError> {
    let value = args.first().map(value_to_string).unwrap_or_default();
    Ok(host_api::array(
        node_module_paths(std::path::Path::new(&value))
            .into_iter()
            .map(|path| Value::String(path.to_string_lossy().into_owned()))
            .collect(),
    ))
}

fn node_module_paths(path: &std::path::Path) -> Vec<std::path::PathBuf> {
    let mut current = path.to_path_buf();
    let mut result = Vec::new();
    loop {
        let candidate = if current
            .file_name()
            .is_some_and(|name| name == "node_modules")
        {
            current.clone()
        } else {
            current.join("node_modules")
        };
        if result.last() != Some(&candidate) {
            result.push(candidate);
        }
        let Some(parent) = current.parent() else {
            break;
        };
        if parent == current {
            break;
        }
        current = parent.to_path_buf();
    }
    result
}

pub fn module_resolve_lookup_paths(args: &[Value]) -> Result<Value, VmError> {
    let value = args.first().map(value_to_string).unwrap_or_default();
    let local = value.starts_with("./")
        || value.starts_with("../")
        || value.starts_with('/')
        || (cfg!(windows) && (value.starts_with(".\\") || value.starts_with("..\\")));
    Ok(host_api::array(vec![Value::String(
        if local { "." } else { "node_modules" }.into(),
    )]))
}

pub fn module_init_paths(state: &Rc<RefCell<HostState>>) -> Result<Value, VmError> {
    let module = module_api(state);
    let paths = execute::get_property(&module, "globalPaths");
    let process = quench_runtime::vm::current_global_object();
    let env = execute::get_property(&execute::get_property(&process, "process"), "env");
    let node_path = execute::get_property(&env, "NODE_PATH");
    let entries = value_to_string(&node_path)
        .split(if cfg!(windows) { ';' } else { ':' })
        .filter(|entry| !entry.is_empty())
        .map(|entry| Value::String(entry.into()))
        .collect::<Vec<_>>();
    let _ = execute::set_array_length_in_place(&paths, 0);
    for (index, value) in entries.into_iter().enumerate() {
        let _ = execute::set_array_element_in_place(&paths, index, value);
    }
    Ok(Value::Undefined)
}

pub fn module_create_require(
    state: &Rc<RefCell<HostState>>,
    args: &[Value],
) -> Result<Value, VmError> {
    let filename = create_require_filename(args.first())?;
    let directory = std::path::Path::new(&filename)
        .parent()
        .unwrap_or_else(|| std::path::Path::new("."))
        .to_string_lossy()
        .into_owned();
    let require = host_api::bound_capability_with_arguments(
        crate::host::capability_ref(crate::registry::SPEC_MODULE_CREATED_REQUIRE),
        vec![Value::String(directory.clone())],
    );
    let resolve = host_api::bound_capability_with_arguments(
        crate::host::capability_ref(crate::registry::SPEC_MODULE_CREATED_RESOLVE),
        vec![Value::String(directory)],
    );
    let _ = execute::set_property_in_place(&require, "resolve", resolve);
    let _ = state;
    Ok(require)
}

pub fn module_stat(args: &[Value]) -> Result<Value, VmError> {
    let path = args.first().map(value_to_string).unwrap_or_default();
    let result = match std::fs::metadata(&path) {
        Ok(metadata) if metadata.is_dir() => 1.0,
        Ok(_) => 0.0,
        Err(_) => -1.0,
    };
    Ok(Value::Number(result))
}

pub fn module_set_source_maps_support(args: &[Value]) -> Result<Value, VmError> {
    if !matches!(args.first(), Some(Value::Boolean(_))) {
        return Err(invalid_type(
            "The \"enabled\" argument must be of type boolean".into(),
        ));
    }
    if let Some(options) = args.get(1) {
        if !matches!(options, Value::Object(_) | Value::ObjectAlias(_)) {
            return Err(invalid_type(
                "The \"options\" argument must be of type object".into(),
            ));
        }
        for name in ["nodeModules", "generatedCode"] {
            let value = execute::get_property(options, name);
            if !matches!(value, Value::Undefined | Value::Boolean(_)) {
                return Err(invalid_type(format!(
                    "The \"options.{name}\" argument must be of type boolean"
                )));
            }
        }
    }
    Ok(Value::Undefined)
}

/// Compile-cache API surface for the Rust engine.  There is no V8 bytecode
/// format to persist, so enabling is an observable no-op with Node's status
/// object rather than a second cache implementation.
pub fn module_enable_compile_cache(args: &[Value]) -> Result<Value, VmError> {
    if let Some(options) = args.first() {
        if matches!(
            options,
            Value::Undefined | Value::Object(_) | Value::ObjectAlias(_)
        ) {
            return Ok(host_api::object(vec![(
                "status".into(),
                Value::Number(3.0),
            )]));
        }
        let error = quench_runtime::builtins::error(
            quench_runtime::ops::Builtin::TypeError,
            &[Value::String(
                "The \"options\" argument must be of type object".into(),
            )],
        );
        return Err(VmError::Thrown(execute::set_property(
            error,
            "code",
            Value::String("ERR_INVALID_ARG_TYPE".into()),
        )));
    }
    Ok(host_api::object(vec![(
        "status".into(),
        Value::Number(3.0),
    )]))
}

pub fn module_get_compile_cache_dir(_: &[Value]) -> Result<Value, VmError> {
    Ok(Value::Undefined)
}

pub fn module_flush_compile_cache(_: &[Value]) -> Result<Value, VmError> {
    Ok(Value::Undefined)
}

fn create_require_filename(value: Option<&Value>) -> Result<String, VmError> {
    let (raw, received) = match value {
        Some(Value::String(value)) => (value.clone(), String::new()),
        Some(value) => match execute::get_property(value, "href") {
            Value::String(href) => (href, String::new()),
            _ => (
                String::new(),
                format!(". Received {}", received_value(value)),
            ),
        },
        None => (String::new(), String::new()),
    };
    let filename = decode_file_url_path(raw.strip_prefix("file://").unwrap_or(&raw))?;
    if raw.starts_with("http:")
        || raw.starts_with("https:")
        || !std::path::Path::new(&filename).is_absolute()
    {
        return Err(VmError::Thrown(host_api::object(vec![
            ("name".into(), Value::String("TypeError".into())),
            ("code".into(), Value::String("ERR_INVALID_ARG_VALUE".into())),
            ("message".into(), Value::String(format!("The argument 'filename' must be a file URL object, file URL string, or absolute path string{}", received))),
        ])));
    }
    Ok(filename)
}

fn decode_file_url_path(path: &str) -> Result<String, VmError> {
    let bytes = path.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] == b'%' {
            let Some((&high, &low)) = bytes.get(index + 1).zip(bytes.get(index + 2)) else {
                return Err(invalid_value(
                    "The argument 'filename' must be a valid file URL".into(),
                ));
            };
            let digit = |byte: u8| match byte {
                b'0'..=b'9' => Some(byte - b'0'),
                b'a'..=b'f' => Some(byte - b'a' + 10),
                b'A'..=b'F' => Some(byte - b'A' + 10),
                _ => None,
            };
            let Some(value) = digit(high)
                .zip(digit(low))
                .map(|(high, low)| high << 4 | low)
            else {
                return Err(invalid_value(
                    "The argument 'filename' must be a valid file URL".into(),
                ));
            };
            out.push(value);
            index += 3;
        } else {
            out.push(bytes[index]);
            index += 1;
        }
    }
    String::from_utf8(out)
        .map_err(|_| invalid_value("The argument 'filename' must be a valid file URL".into()))
}

fn received_value(value: &Value) -> String {
    match value {
        Value::Object(_) => "{}".into(),
        Value::Array(_) => "[]".into(),
        _ => value_to_string(value),
    }
}

pub fn module_created_require(
    state: &Rc<RefCell<HostState>>,
    args: &[Value],
) -> Result<Value, VmError> {
    let Some(Value::String(base)) = args.first() else {
        return Err(VmError::NotCallable);
    };
    let spec = args.get(1).cloned().unwrap_or(Value::Undefined);
    state.borrow_mut().dir_stack.push(base.clone());
    let result = require(state, &[spec]);
    state.borrow_mut().dir_stack.pop();
    result
}

pub fn module_created_resolve(
    state: &Rc<RefCell<HostState>>,
    args: &[Value],
) -> Result<Value, VmError> {
    let Some(Value::String(base)) = args.first() else {
        return Err(VmError::NotCallable);
    };
    let spec = args.get(1).cloned().unwrap_or(Value::Undefined);
    state.borrow_mut().dir_stack.push(base.clone());
    let result = resolve_require(state, &[spec]);
    state.borrow_mut().dir_stack.pop();
    result
}

fn require_impl(state: &Rc<RefCell<HostState>>, args: &[Value]) -> Result<Value, VmError> {
    let spec = args
        .first()
        .map(|value| match execute::get_property(value, "href") {
            Value::String(href) => href,
            _ => value_to_string(value),
        })
        .unwrap_or_default();
    let spec = if let Some(path) = spec.strip_prefix("file://") {
        decode_file_url_path(path).unwrap_or(spec)
    } else {
        spec
    };
    if STATIC_ESM_LOAD.with(|flag| flag.get())
        && !spec.starts_with("node:")
        && !std::path::Path::new(&spec).extension().is_some()
    {
        let base = state
            .borrow()
            .dir_stack
            .last()
            .cloned()
            .unwrap_or_else(|| ".".to_string());
        let direct = std::path::Path::new(&base).join(&spec);
        let package_dir = node_module_paths(std::path::Path::new(&base))
            .into_iter()
            .map(|root| root.join(&spec))
            .find(|candidate| candidate.is_dir());
        if direct.is_dir() || (spec.contains('/') && package_dir.is_some()) {
            return Err(invalid_value(format!(
                "Directory import '{}' is not supported",
                spec
            )));
        }
    }
    let vfs_enabled = {
        let global = quench_runtime::vm::current_global_object();
        let process = execute::get_property(&global, "process");
        let exec_argv = execute::get_property(&process, "execArgv");
        matches!(exec_argv, Value::Array(ref values) if (0..values.logical_len()).any(|i| matches!(values.get(i), Some(Value::String(flag)) if flag == "--experimental-vfs")))
    };
    if spec == "node:vfs" && !vfs_enabled {
        return Err(unknown_builtin(&spec));
    }
    if spec == "vfs" {
        return Err(not_found(&spec));
    }
    if matches!(spec.as_str(), "punycode" | "node:punycode") {
        let global = quench_runtime::vm::current_global_object();
        let module = execute::get_property(&global, "__quenchPunycode");
        if matches!(module, Value::Object(_) | Value::ObjectAlias(_)) {
            state
                .borrow_mut()
                .module_cache
                .insert("punycode".into(), module.clone());
            return Ok(module);
        }
    }
    // The Node test helpers are one host-owned resource.  Resolve every
    // spelling of the common entry point and tmpdir helper before filesystem
    // resolution; otherwise `common/index.js` loads a second JS copy whose
    // `./tmpdir` points at the checkout's `.tmp.0` directory.
    if spec == "../common"
        || spec == "../common/index"
        || ((spec.contains("node/test/common")
            || spec.contains("tests/node/common")
            || spec.contains("node/common"))
            && (spec.ends_with("/common") || spec.ends_with("/common/index")))
    {
        let common = quench_runtime::execute::get_property(
            &quench_runtime::vm::current_global_object(),
            "__nodeCommon",
        );
        if matches!(common, Value::Object(_) | Value::ObjectAlias(_)) {
            if matches!(execute::get_property(&common, "skipIfPerfettoEnabled"), Value::Undefined) {
                let _ = execute::set_property_in_place(
                    &common,
                    "skipIfPerfettoEnabled",
                    crate::host::capability(crate::registry::SPEC_COMMON_SKIP_IF_PERFETTO),
                );
            }
            return Ok(common);
        }
    }
    if spec == "../common/tmpdir"
        || spec == "../common/tmpdir.js"
        || spec.ends_with("node/test/common/tmpdir")
        || spec.ends_with("node/test/common/tmpdir.js")
        || spec.ends_with("tests/node/common/tmpdir")
        || spec.ends_with("tests/node/common/tmpdir.js")
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
    // Test-module mocks shadow the ordinary module cache while registered.
    // Keeping this check before cache lookup gives mocks the same precedence
    // as Node's loader hooks, while reset restores the original cache entry.
    if crate::modules::test::mock_has_unappliable_default(&spec) {
        return Err(invalid_value("Cannot create mock".into()));
    }
    if let Some(mock) = crate::modules::test::mocked_module(&spec) {
        if crate::modules::test::mock_module_cache(&spec) {
            let key = format!(
                "\0mock:{}",
                crate::modules::test::canonical_mock_specifier(&spec)
            );
            if let Some(cached) = state.borrow().module_cache.get(&key) {
                return Ok(cached.clone());
            }
            state.borrow_mut().module_cache.insert(key, mock.clone());
        }
        return Ok(mock);
    }
    // CommonJS exposes a mutable cache object.  Bare names consult it before
    // builtin construction; `node:` spellings intentionally bypass it.
    if !spec.starts_with("node:") {
        if let Some(exports) = cache_exports(state, &spec) {
            return Ok(exports);
        }
    }
    if matches!(
        spec.as_str(),
        "internal/child_process" | "internal/child_process.js" | "node:internal/child_process"
    ) {
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
                "getValidStdio".into(),
                crate::host::capability(crate::registry::SPEC_CP_GET_VALID_STDIO),
            ),
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
    if matches!(
        spec.as_str(),
        "internal/streams/add-abort-signal"
            | "internal/streams/add-abort-signal.js"
            | "node:internal/streams/add-abort-signal"
    ) {
        let helper = quench_runtime::host_api::object(vec![(
            "addAbortSignalNoValidate".into(),
            quench_runtime::host_api::capability_function(quench_runtime::ops::HostCapabilityRef {
                realm: quench_runtime::ops::RealmId::ROOT,
                kind: quench_runtime::ops::HostCapabilityKind::Custom(
                    crate::registry::SPEC_STREAM_ADD_ABORT_SIGNAL_NO_VALIDATE.cap,
                ),
            }),
        )]);
        return Ok(helper);
    }
    if matches!(
        spec.as_str(),
        "internal/streams/end-of-stream"
            | "internal/streams/end-of-stream.js"
            | "node:internal/streams/end-of-stream"
    ) {
        let helper = quench_runtime::host_api::object(vec![(
            "kEosNodeSynchronousCallback".into(),
            Value::String("Symbol(kEosNodeSynchronousCallback)".into()),
        )]);
        return Ok(helper);
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
    if matches!(spec.as_str(), "assert" | "node:assert") {
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
        let promises = stream_promises_for(value.clone());
        let value = execute::set_property(value, "promises", promises.clone());
        state
            .borrow_mut()
            .module_cache
            .insert("stream/promises".to_string(), promises);
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
    if spec.starts_with("node:internal/") && !is_builtin_name(&spec) {
        return Err(unknown_builtin(&spec));
    }
    if let Some(ns) = resolve(state, &spec) {
        state
            .borrow_mut()
            .module_cache
            .insert(cache_key, ns.clone());
        return Ok(ns);
    }
    if spec.starts_with("node:") {
        return Err(unknown_builtin(&spec));
    }
    load_file_module(state, &spec)
}

/// CommonJS file loader: resolve, cache, wrap, execute, return exports.
fn load_file_module(state: &Rc<RefCell<HostState>>, spec: &str) -> Result<Value, VmError> {
    let path = resolve_path(state, spec)?;
    let requested = if std::path::Path::new(spec).is_absolute() {
        std::path::PathBuf::from(spec)
    } else {
        let base = state
            .borrow()
            .dir_stack
            .last()
            .cloned()
            .unwrap_or_else(|| "/".into());
        std::path::Path::new(&base).join(spec)
    };
    let requested = normalize_path(requested);
    let key = path.to_string_lossy().into_owned();
    if let Some(exports) = cache_exports(state, &key) {
        append_cached_child(state, &path);
        return Ok(exports);
    }
    if let Some(cached) = state.borrow().module_cache.get(&key) {
        append_cached_child(state, &path);
        return Ok(cached.clone());
    }
    if path.extension().and_then(|extension| extension.to_str()) == Some("node") {
        return Err(native_addon_error(&path));
    }
    let source = {
        let global = quench_runtime::vm::current_global_object();
        let fs_module = execute::get_property(&global, "__nodeFs");
        let reader = execute::get_property(&fs_module, "readFileSync");
        if quench_runtime::is_callable(&reader) {
            match execute::call(
                &reader,
                &fs_module,
                &[
                    Value::String(path.to_string_lossy().into_owned()),
                    Value::String("utf8".into()),
                ],
            ) {
                Ok(Value::String(source)) => source,
                Ok(value) => execute::to_js_string(&value).unwrap_or_else(|_| String::new()),
                Err(_) => std::fs::read_to_string(&path)
                    .map_err(|_| VmError::EvalError(format!("Cannot find module '{spec}'")))?,
            }
        } else {
            std::fs::read_to_string(&path)
                .map_err(|_| VmError::EvalError(format!("Cannot find module '{spec}'")))?
        }
    };
    let is_module_source = path.extension().and_then(|extension| extension.to_str()) == Some("mjs")
        || (path.extension().and_then(|extension| extension.to_str()) == Some("js")
            && package_type_is_module(&path));
    if is_module_source {
        let experimental_require = quench_runtime::execute::get_property(
            &quench_runtime::vm::current_global_object(),
            "process",
        );
        let experimental_require = DYNAMIC_ESM_LOAD.with(|flag| flag.get())
            || matches!(
                quench_runtime::execute::get_property(&experimental_require, "execArgv"),
                Value::Array(ref values)
                    if (0..values.logical_len()).any(|index| matches!(
                        values.get(index),
                        Some(Value::String(flag)) if flag == "--experimental-require-module"
                    ))
            );
        if experimental_require {
            let transformed = crate::esm_imports::transform_esm_module(&source);
            let empty_exports = host_api::object(Vec::new());
            cache_module(state, &path, &empty_exports);
            let exports = execute_module(state, &path, &transformed)?;
            state.borrow_mut().module_cache.insert(key, exports.clone());
            cache_module(state, &path, &exports);
            return Ok(exports);
        }
        return Err(require_esm_error(&path));
    }
    if path.extension().and_then(|extension| extension.to_str()) == Some("json") {
        let value = parse_json_module(&source, &path.to_string_lossy())?;
        state.borrow_mut().module_cache.insert(key, value.clone());
        cache_module(state, &path, &value);
        return Ok(value);
    }
    let empty_exports = host_api::object(Vec::new());
    cache_module(state, &path, &empty_exports);
    if let Some(exports) = load_custom_extension(state, &path, &source, !requested.is_file())? {
        state.borrow_mut().module_cache.insert(key, exports.clone());
        cache_module(state, &path, &exports);
        return Ok(exports);
    }
    let exports = match execute_module(state, &path, &source) {
        Ok(exports) => exports,
        Err(error) => {
            clear_cache_entry(state, &path);
            return Err(error);
        }
    };
    state.borrow_mut().module_cache.insert(key, exports.clone());
    cache_module(state, &path, &exports);
    Ok(exports)
}

fn native_addon_error(path: &std::path::Path) -> VmError {
    let error = quench_runtime::builtins::error(
        quench_runtime::ops::Builtin::Error,
        &[Value::String(format!("file too short: {}", path.display()))],
    );
    VmError::Thrown(execute::set_property(
        error,
        "code",
        Value::String("ERR_DLOPEN_FAILED".into()),
    ))
}

fn package_type_is_module(path: &std::path::Path) -> bool {
    let mut directory = path.parent();
    while let Some(current) = directory {
        let manifest = current.join("package.json");
        if let Ok(source) = std::fs::read_to_string(manifest) {
            let Ok(value) = serde_json::from_str::<serde_json::Value>(&source) else {
                return false;
            };
            // The repository manifest is private tooling metadata, not the
            // package scope of the Node fixtures executed by this host.  Its
            // module type must not silently turn every fixture `.js` file into an
            // ES module; fixture packages still opt in through their own nearest
            // manifest, exactly as Node's package-boundary walk does.
            if value.get("private").and_then(serde_json::Value::as_bool) == Some(true)
                && value.get("name").is_none()
            {
                directory = current.parent();
                continue;
            }
            return value.get("type").and_then(serde_json::Value::as_str) == Some("module");
        }
        directory = current.parent();
    }
    false
}

fn clear_cache_entry(state: &Rc<RefCell<HostState>>, path: &std::path::Path) {
    let _ = execute::set_property_in_place(
        &cache_object(state),
        &path.to_string_lossy(),
        Value::Undefined,
    );
    state
        .borrow_mut()
        .module_cache
        .remove(&path.to_string_lossy().into_owned());
}

fn load_custom_extension(
    state: &Rc<RefCell<HostState>>,
    path: &std::path::Path,
    _source: &str,
    appended: bool,
) -> Result<Option<Value>, VmError> {
    if path
        .file_name()
        .and_then(|name| name.to_str())
        .is_some_and(|name| name.starts_with('.') && !name[1..].contains('.'))
    {
        // A bare dotfile (e.g. `.bar`) is not treated as an extension-bearing
        // module by Node's legacy loader.
        return Ok(None);
    }
    let extensions = execute::get_property(&module_api(state), "_extensions");
    let suffix = path.to_string_lossy();
    let mut matches = execute::own_enumerable_keys(&extensions)
        .into_iter()
        .filter(|key| suffix.ends_with(key))
        .collect::<Vec<_>>();
    if appended {
        matches.sort_by_key(|key| std::cmp::Reverse(key.len()));
    } else {
        matches.sort_by_key(|key| key.len());
    }
    let Some(extension) = matches.into_iter().next() else {
        return Ok(None);
    };
    let handler = execute::get_property(&extensions, &extension);
    if !quench_runtime::is_callable(&handler) {
        return Ok(None);
    }
    let module = host_api::object(vec![
        ("exports".into(), host_api::object(Vec::new())),
        (
            "filename".into(),
            Value::String(suffix.clone().into_owned()),
        ),
    ]);
    execute::call(
        &handler,
        &Value::Undefined,
        &[module.clone(), Value::String(suffix.into_owned())],
    )?;
    Ok(Some(execute::get_property(&module, "exports")))
}

fn cache_object(state: &Rc<RefCell<HostState>>) -> Value {
    execute::get_property(&module_api(state), "_cache")
}

fn cache_exports(state: &Rc<RefCell<HostState>>, key: &str) -> Option<Value> {
    let entry = execute::get_property(&cache_object(state), key);
    match entry {
        Value::Undefined | Value::Null => None,
        value => Some(execute::get_property(&value, "exports")),
    }
}

fn cache_module(state: &Rc<RefCell<HostState>>, path: &std::path::Path, exports: &Value) {
    let cache = cache_object(state);
    let entry = match execute::get_property(&cache, &path.to_string_lossy()) {
        value @ (Value::Object(_) | Value::ObjectAlias(_)) => {
            let _ = execute::set_property_in_place(&value, "exports", exports.clone());
            value
        }
        _ => host_api::object(vec![
            ("exports".into(), exports.clone()),
            (
                "filename".into(),
                Value::String(path.to_string_lossy().into_owned()),
            ),
            ("children".into(), host_api::array(Vec::new())),
        ]),
    };
    let _ = execute::set_property_in_place(&cache_object(state), &path.to_string_lossy(), entry);
}

fn append_cached_child(state: &Rc<RefCell<HostState>>, child: &std::path::Path) {
    let Some(parent) = state.borrow().module_stack.last().cloned() else {
        return;
    };
    let parent_entry = execute::get_property(&cache_object(state), &parent);
    let children = execute::get_property(&parent_entry, "children");
    let Value::Array(array) = children else {
        return;
    };
    let child_entry = execute::get_property(&cache_object(state), &child.to_string_lossy());
    for index in 0..array.logical_len() {
        let existing = array.get(index).unwrap_or(Value::Undefined);
        if value_to_string(&execute::get_property(&existing, "filename")) == child.to_string_lossy()
        {
            return;
        }
    }
    let index = array.logical_len();
    let children_value = Value::Array(array);
    let _ = execute::set_array_length_in_place(&children_value, index + 1);
    let _ = execute::set_array_element_in_place(&children_value, index, child_entry);
}

fn parse_json_module(source: &str, filename: &str) -> Result<Value, VmError> {
    let json = serde_json::from_str::<serde_json::Value>(source).map_err(|error| {
        VmError::Thrown(quench_runtime::builtins::error(
            quench_runtime::ops::Builtin::SyntaxError,
            &[Value::String(format!("{filename}: {error}"))],
        ))
    })?;
    Ok(json_value(json))
}

fn json_value(value: serde_json::Value) -> Value {
    match value {
        serde_json::Value::Null => Value::Null,
        serde_json::Value::Bool(value) => Value::Boolean(value),
        serde_json::Value::Number(value) => Value::Number(value.as_f64().unwrap_or(f64::NAN)),
        serde_json::Value::String(value) => Value::String(value),
        serde_json::Value::Array(values) => {
            host_api::array(values.into_iter().map(json_value).collect())
        }
        serde_json::Value::Object(values) => host_api::object(
            values
                .into_iter()
                .map(|(key, value)| (key, json_value(value)))
                .collect(),
        ),
    }
}

fn resolve_path(state: &Rc<RefCell<HostState>>, spec: &str) -> Result<std::path::PathBuf, VmError> {
    let base = state
        .borrow()
        .dir_stack
        .last()
        .cloned()
        .unwrap_or_else(|| "/".to_string());
    resolve_path_from_base(state, spec, &base)
}

fn resolve_base(
    state: &Rc<RefCell<HostState>>,
    options: Option<&Value>,
) -> Result<String, VmError> {
    let default = state
        .borrow()
        .dir_stack
        .last()
        .cloned()
        .unwrap_or_else(|| "/".to_string());
    let Some(options) = options else {
        return Ok(default);
    };
    let paths = execute::get_property(options, "paths");
    let Value::Array(paths) = paths else {
        return Ok(default);
    };
    let first = execute::get_property(&Value::Array(paths), "0");
    let Value::String(path) = first else {
        return Err(VmError::Thrown(host_api::object(vec![
            ("name".into(), Value::String("TypeError".into())),
            ("code".into(), Value::String("ERR_INVALID_ARG_TYPE".into())),
            (
                "message".into(),
                Value::String("The \"paths\" argument must be an array of strings".into()),
            ),
        ])));
    };
    let cwd = state.borrow().process.cwd.clone();
    Ok(cwd.join(path).to_string_lossy().into_owned())
}

fn resolve_path_from_base(
    state: &Rc<RefCell<HostState>>,
    spec: &str,
    base: &str,
) -> Result<std::path::PathBuf, VmError> {
    // oxc-resolver handles extension probing (.js), directory index files
    // (index.js), package.json mains, and the node_modules walk (with the
    // `exports`/`imports` maps and conditional exports) — the canonical
    // Node resolution algorithm. Relative and bare (npm package)
    // specifiers both resolve from the requiring module's directory.
    let resolver = oxc_resolver::Resolver::new(oxc_resolver::ResolveOptions {
        extensions: vec![".js".into(), ".json".into(), ".cjs".into()],
        main_files: vec!["index".into()],
        condition_names: vec!["node".into(), "require".into(), "default".into()],
        ..oxc_resolver::ResolveOptions::default()
    });
    let requested = if std::path::Path::new(spec).is_absolute() {
        std::path::PathBuf::from(spec)
    } else {
        std::path::Path::new(base).join(spec)
    };
    let requested = normalize_path(requested);
    // A path ending in `..` is an explicit directory traversal. Node resolves
    // its package/index before probing a sibling with the same basename and a
    // file extension (the `module-stub` fixture exercises this distinction).
    let directory_first = spec.trim_end_matches('/').ends_with("..");
    // The compatibility fixtures retain upstream `../../tests/node/test`
    // specifiers even though they execute from `tests/node-compat/stage-*`.
    // Resolve those canonical Node test helpers against the checkout root;
    // ordinary relative requests continue through the normal resolver below.
    if spec.contains("tests/node/test/common/") {
        let helper = if spec.ends_with("/fixtures") || spec.ends_with("/fixtures.js") {
            "fixtures.js"
        } else if spec.ends_with("/tick") || spec.ends_with("/tick.js") {
            "tick.js"
        } else {
            ""
        };
        if !helper.is_empty() {
            let candidate = state
                .borrow()
                .process
                .cwd
                .join("tests/node/test/common")
                .join(helper);
            if candidate.is_file() {
                return Ok(canonical_path(candidate));
            }
        }
    }
    let extensions = state.borrow().module_extensions.clone().unwrap_or_else(|| {
        let global = quench_runtime::vm::current_global_object();
        execute::get_property(&execute::get_property(&global, "require"), "extensions")
    });
    let mut custom = execute::own_enumerable_keys(&extensions)
        .into_iter()
        .filter(|key| key.starts_with('.'))
        .collect::<Vec<_>>();
    custom.sort_by_key(|key| std::cmp::Reverse(key.len()));
    if directory_first && requested.is_dir() {
        for index in ["index.js", "index.json", "index.cjs"] {
            let candidate = requested.join(index);
            if candidate.is_file() {
                return Ok(canonical_path(candidate));
            }
        }
    }
    for extension in custom {
        let candidate =
            std::path::PathBuf::from(format!("{}{}", requested.to_string_lossy(), extension));
        if candidate.is_file() {
            return Ok(canonical_path(candidate));
        }
    }
    if requested.is_file() {
        return Ok(canonical_path(requested));
    }
    for extension in [".js", ".json", ".cjs"] {
        let candidate =
            std::path::PathBuf::from(format!("{}{}", requested.to_string_lossy(), extension));
        if candidate.is_file() {
            return Ok(canonical_path(candidate));
        }
    }
    if requested.is_dir() {
        for index in ["index.js", "index.json", "index.cjs"] {
            let candidate = requested.join(index);
            if candidate.is_file() {
                return Ok(canonical_path(candidate));
            }
        }
    }
    // Module._stat is an intentional host escape hatch: VFS providers can
    // report virtual files that do not exist on the host filesystem.  Honor
    // the current module object's override before falling back to the native
    // resolver, preserving ordinary MODULE_NOT_FOUND behavior otherwise.
    if let Some(module) = state.borrow().module_api.clone() {
        let stat = execute::get_property(&module, "_stat");
        if quench_runtime::is_callable(&stat) {
            if let Ok(value) = execute::call(
                &stat,
                &module,
                &[Value::String(requested.to_string_lossy().into_owned())],
            ) {
                if matches!(value, Value::Number(number) if number == 0.0) {
                    return Ok(requested);
                }
            }
        }
    }
    if !spec.starts_with('.') && !std::path::Path::new(spec).is_absolute() {
        if let Some(path) = resolve_global_module_path(state, spec) {
            return Ok(path);
        }
    }
    if !spec.starts_with('.') && !std::path::Path::new(spec).is_absolute() {
        let mut directory = std::path::Path::new(base).to_path_buf();
        loop {
            let candidate = directory.join("node_modules").join(spec);
            if let Some(path) = probe_global_module_candidate(&candidate) {
                return Ok(path);
            }
            let Some(parent) = directory.parent() else {
                break;
            };
            if parent == directory {
                break;
            }
            directory = parent.to_path_buf();
        }
    }
    if let Ok(resolution) = resolver.resolve(std::path::Path::new(base), spec) {
        return Ok(canonical_path(resolution.into_path_buf()));
    }
    // Node's legacy lookup also treats a file directly under node_modules as
    // a package (for example `node_modules/bar.js` for request `bar`).  The
    // resolver intentionally models package directories, so keep this small
    // data-driven fallback at the host boundary.
    if !spec.starts_with('.') && !std::path::Path::new(spec).is_absolute() {
        let mut directory = std::path::Path::new(base).to_path_buf();
        loop {
            let candidate = directory.join("node_modules").join(spec);
            if let Some(path) = probe_global_module_candidate(&candidate) {
                return Ok(path);
            }
            if let Some(parent) = directory.parent() {
                if parent == directory {
                    break;
                }
                directory = parent.to_path_buf();
            } else {
                break;
            }
        }
    }
    Err(not_found(spec))
}

fn resolve_global_module_path(
    state: &Rc<RefCell<HostState>>,
    spec: &str,
) -> Option<std::path::PathBuf> {
    let paths = execute::get_property(&module_api(state), "globalPaths");
    let cwd = state.borrow().process.cwd.clone();
    for key in execute::own_enumerable_keys(&paths) {
        let root = value_to_string(&execute::get_property(&paths, &key));
        let root = std::path::Path::new(&root);
        let root = if root.is_absolute() {
            root.to_path_buf()
        } else {
            std::path::Path::new(&cwd).join(root)
        };
        let candidate = root.join(spec);
        if let Some(path) = probe_global_module_candidate(&candidate) {
            return Some(path);
        }
    }
    None
}

fn probe_global_module_candidate(candidate: &std::path::Path) -> Option<std::path::PathBuf> {
    for path in [
        candidate.to_path_buf(),
        candidate.with_extension("js"),
        candidate.with_extension("json"),
        candidate.with_extension("cjs"),
    ] {
        if path.is_file() {
            return Some(canonical_path(path));
        }
    }
    if candidate.is_dir() {
        if let Ok(source) = std::fs::read_to_string(candidate.join("package.json")) {
            if let Ok(manifest) = serde_json::from_str::<serde_json::Value>(&source) {
                if let Some(main) = manifest.get("main").and_then(serde_json::Value::as_str) {
                    let target = candidate.join(main);
                    for path in [
                        target.clone(),
                        target.with_extension("js"),
                        target.with_extension("json"),
                        target.with_extension("cjs"),
                        target.join("index.js"),
                        target.join("index.json"),
                        target.join("index.cjs"),
                    ] {
                        if path.is_file() {
                            return Some(canonical_path(normalize_path(path)));
                        }
                    }
                }
            }
        }
        for path in [
            candidate.join("index.js"),
            candidate.join("index.json"),
            candidate.join("index.cjs"),
        ] {
            if path.is_file() {
                return Some(canonical_path(path));
            }
        }
    }
    None
}

fn canonical_path(path: std::path::PathBuf) -> std::path::PathBuf {
    // Node preserves the logical `/tmp` spelling exposed by process.cwd()
    // even when the host mounts it through `/private/tmp` (macOS). Keep that
    // observable path stable while retaining realpath behavior elsewhere.
    if path.starts_with(std::path::Path::new("/tmp")) {
        return path;
    }
    if let Ok(relative) = path.strip_prefix(std::path::Path::new("/private/tmp")) {
        return std::path::Path::new("/tmp").join(relative);
    }
    std::fs::canonicalize(&path).unwrap_or(path)
}

fn normalize_path(path: std::path::PathBuf) -> std::path::PathBuf {
    let mut normalized = std::path::PathBuf::new();
    for component in path.components() {
        match component {
            std::path::Component::CurDir => {}
            std::path::Component::ParentDir => {
                normalized.pop();
            }
            component => normalized.push(component.as_os_str()),
        }
    }
    normalized
}

fn not_found(spec: &str) -> VmError {
    let mut error = quench_runtime::builtins::error(
        quench_runtime::ops::Builtin::Error,
        &[Value::String(format!("Cannot find module '{spec}'"))],
    );
    let _ = execute::set_property_in_place(
        &mut error,
        "code",
        Value::String("MODULE_NOT_FOUND".into()),
    );
    VmError::Thrown(error)
}

fn invalid_value(message: String) -> VmError {
    let error = quench_runtime::builtins::error(
        quench_runtime::ops::Builtin::TypeError,
        &[Value::String(message.clone())],
    );
    VmError::Thrown(execute::set_property(
        error,
        "code",
        Value::String("ERR_INVALID_ARG_VALUE".into()),
    ))
}

fn invalid_type(message: String) -> VmError {
    let error = quench_runtime::builtins::error(
        quench_runtime::ops::Builtin::TypeError,
        &[Value::String(message)],
    );
    VmError::Thrown(execute::set_property(
        error,
        "code",
        Value::String("ERR_INVALID_ARG_TYPE".into()),
    ))
}

fn require_esm_error(path: &std::path::Path) -> VmError {
    let error = quench_runtime::builtins::error(
        quench_runtime::ops::Builtin::Error,
        &[Value::String(format!(
            "require() of ES Module {} not supported. Use dynamic import() which is available in all CommonJS modules.",
            path.display()
        ))],
    );
    VmError::Thrown(execute::set_property(
        error,
        "code",
        Value::String("ERR_REQUIRE_ESM".into()),
    ))
}

fn unknown_builtin(spec: &str) -> VmError {
    let name = spec.strip_prefix("node:").unwrap_or(spec);
    let error = quench_runtime::builtins::error(
        quench_runtime::ops::Builtin::Error,
        &[Value::String(format!(
            "Error [ERR_UNKNOWN_BUILTIN_MODULE]: No such built-in module: {name}"
        ))],
    );
    VmError::Thrown(execute::set_property(
        error,
        "code",
        Value::String("ERR_UNKNOWN_BUILTIN_MODULE".into()),
    ))
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
        .map_err(|errors| module_syntax_error(&filename, source, &errors))?;
    let context = quench_runtime::vm::current_context()
        .as_ref()
        .clone()
        .with_source_text(source.to_owned())
        .with_compiled_source_text(source.to_owned());
    // Re-entrant execution: `execute_with_context` would reset the
    // runtime's locals state and corrupt the frame that called `require`.
    let mut registers = quench_runtime::register_file::RegisterFile::new();
    quench_runtime::vm::execute_code_in_place_context(program.code(), &mut registers, &context)?;
    let module = module.unwrap_or(Value::Undefined);
    quench_runtime::execute::get_property_result(&module, "exports")
}

/// Turn a nested CommonJS parse failure into the same observable SyntaxError
/// shape as Node's module loader.  The private arrow-message slot is consumed
/// by `internal/util/inspect` and must carry the original filename/line even
/// though the host keeps the parser and reducer entirely in Rust.
fn module_syntax_error(filename: &str, source: &str, errors: &[String]) -> VmError {
    let message = errors.join("; ");
    let error = quench_runtime::builtins::error(
        quench_runtime::ops::Builtin::SyntaxError,
        &[Value::String(message.clone())],
    );
    let _ = execute::set_property_in_place(
        &error,
        "Symbol.node:arrowMessage\0internal",
        Value::String(format!("{filename}:1")),
    );
    let first_line = source.lines().next().unwrap_or_default();
    let stack = format!("{filename}:1\n{first_line}\n ^\n\n{message}\n    at {filename}:1:1");
    let _ = execute::set_property_in_place(&error, "stack", Value::String(stack));
    VmError::Thrown(error)
}

/// Prepare `source` as a CJS module: records the pending module
/// record and returns the wrapped source. The caller reduces and
/// executes the result — in-place for nested `require`, in a fresh
/// frame for the main script (see `quench-node-test`'s runner).
pub fn wrap_cjs(state: &Rc<RefCell<HostState>>, filename: &str, source: &str) -> String {
    let exports = host_api::object(vec![]);
    let cached_children = execute::get_property(
        &execute::get_property(&cache_object(state), filename),
        "children",
    );
    let children = if matches!(cached_children, Value::Array(_)) {
        cached_children
    } else {
        host_api::array(Vec::new())
    };
    let module = host_api::object(vec![
        ("exports".to_string(), exports),
        ("filename".to_string(), Value::String(filename.to_string())),
        ("children".to_string(), children),
        ("parent".to_string(), Value::Null),
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
    // Materialize the canonical worker messaging constructors before module
    // code runs. Node exposes these constructors globally as well as from
    // `worker_threads`; keeping one module-derived identity avoids the
    // bootstrap placeholder with inert ports.
    // Keep a source-level strict directive in directive-prologue position. The
    // wrapper's host setup is observable, but must not accidentally turn a
    // strict CommonJS fixture into sloppy code by preceding its directive.
    let mut directive = "";
    if has_cjs_strict_directive(source) {
        directive = "\"use strict\";\n";
    }
    format!("__quench_cjs_wrap__(function (exports, require, module, __filename, __dirname) {{\n{directive}const __quench_worker_threads = require('worker_threads');\nconst __quench_preserved_globals = ['MessageChannel','MessagePort','worker_threads','TypeMismatchError','QuotaExceededError','__nodeCurrentAsyncResource','__nodeCallChecks'];\nfor (let __i = 0; __i < __quench_preserved_globals.length; __i++) {{ const __name = __quench_preserved_globals[__i]; if (__name in globalThis) Object.defineProperty(globalThis, __name, {{ configurable: true, enumerable: false, writable: true, value: globalThis[__name] }}); }}\nObject.defineProperty(globalThis, 'MessageChannel', {{ configurable: true, enumerable: false, writable: true, value: __quench_worker_threads.MessageChannel }});\nObject.defineProperty(globalThis, 'MessagePort', {{ configurable: true, enumerable: false, writable: true, value: __quench_worker_threads.MessagePort }});\n{source}\n}})")
}

fn has_cjs_strict_directive(source: &str) -> bool {
    let mut block_comment = false;
    for line in source.lines() {
        let mut text = line.trim();
        if block_comment {
            let Some((_, rest)) = text.split_once("*/") else {
                continue;
            };
            block_comment = false;
            text = rest.trim();
        }
        if text.is_empty() || text.starts_with("//") {
            continue;
        }
        if text.starts_with("/*") {
            if !text.contains("*/") {
                block_comment = true;
            }
            continue;
        }
        return text.starts_with("'use strict'") || text.starts_with("\"use strict\"");
    }
    false
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
    let require = host_api::bound_capability_with_arguments(
        crate::host::capability_ref(crate::registry::SPEC_MODULE_CREATED_REQUIRE),
        vec![Value::String(pending.dirname.clone())],
    );
    let require = execute::set_property(require, "resolve", resolve_capability());
    let require = execute::set_property(
        require,
        "cache",
        execute::get_property(&module_api(state), "_cache"),
    );
    let require = execute::set_property(
        require,
        "extensions",
        execute::get_property(&module_api(state), "_extensions"),
    );
    let result = quench_runtime::vm::call_value(
        function,
        &Value::Undefined,
        &[
            exports,
            require,
            pending.module.clone(),
            Value::String(pending.filename.clone()),
            Value::String(pending.dirname.clone()),
        ],
    );
    state.borrow_mut().module_stack.pop();
    state.borrow_mut().dir_stack.pop();
    result
}

fn resolve(state: &Rc<RefCell<HostState>>, spec: &str) -> Option<Value> {
    let name = spec.strip_prefix("node:").unwrap_or(spec);
    // `test` is an ordinary userland package name; only the explicit
    // `node:test` spelling is builtin.  This distinction is observable when
    // a node_modules package shadows the test runner entry point.
    if name == "test" && spec != "node:test" {
        return None;
    }
    match name {
        "console" => state
            .borrow()
            .console_module
            .clone()
            .or_else(|| Some(crate::modules::console::build_value())),
        "process" => {
            let process = state.borrow();
            process.process_module.clone().or_else(|| {
                Some(crate::modules::process::build_with_title(
                    &process.process.argv,
                    &process.process.exec_path,
                    &process.process.title,
                ))
            })
        }
        "module" => Some(module_api(state)),
        "buffer" => Some(crate::modules::buffer::build_module()),
        // `perf_hooks` shares the process-wide WHATWG Performance object
        // installed during bootstrap.  Keep the namespace Rust-owned so
        // `require('perf_hooks')` and `require('node:perf_hooks')` preserve
        // identity instead of falling through to an empty placeholder.
        "perf_hooks" => {
            let global = quench_runtime::vm::current_global_object();
            let performance = execute::get_property(&global, "performance");
            let observer = execute::get_property(&global, "__nodePerfHooks");
            let observer_ctor = execute::get_property(&observer, "PerformanceObserver");
            let timerify = execute::get_property(&observer, "timerify");
            let histogram = execute::get_property(&observer, "createHistogram");
            Some(crate::host::namespace_object_from_pairs(vec![
                ("performance".into(), performance),
                ("PerformanceObserver".into(), observer_ctor),
                ("timerify".into(), timerify),
                ("createHistogram".into(), histogram),
            ]))
        }
        "ajv" => Some(crate::modules::npm::ajv_module()),
        "chalk" => Some(crate::modules::npm::chalk_module()),
        "prettier" => Some(crate::modules::npm::prettier_module()),
        "internal/buffer" => Some(crate::host::namespace_object_from_pairs(vec![(
            "utf8Write".to_string(),
            crate::host::capability(crate::registry::SPEC_INTERNAL_BUFFER_UTF8_WRITE),
        )])),
        "internal/encoding" => Some(crate::host::namespace_object_from_pairs(vec![(
            "getEncodingFromLabel".into(),
            crate::host::capability(crate::registry::SPEC_INTERNAL_ENCODING_GET_LABEL),
        )])),
        "_http_server" => Some(crate::host::namespace_object_from_pairs(vec![(
            "kConnectionsCheckingInterval".into(),
            Value::String("Symbol.kConnectionsCheckingInterval\0quench".into()),
        )])),
        "internal/js_stream_socket" => {
            let ctor = crate::host::capability(crate::registry::SPEC_INTERNAL_JS_STREAM);
            Some(execute::set_property(ctor.clone(), "StreamWrap", ctor))
        }
        "internal/net" => Some(crate::modules::net::internal_module()),
        "internal/assert" => Some(crate::host::capability(crate::registry::SPEC_ASSERT_OK)),
        "internal/assert/myers_diff" => Some(crate::host::namespace_object_from_pairs(vec![(
            "myersDiff".into(),
            crate::host::capability(crate::registry::SPEC_INTERNAL_ASSERT_MYERS_DIFF),
        )])),
        "internal/http2/util" => Some(crate::modules::http2_util::module()),
        "http2" => Some(http2_module_value()),
        "internal/fs/promises" => Some(crate::modules::fs::internal_file_handle_module()),
        "internal/fs/utils" => Some(crate::host::namespace_object_from_pairs(vec![
            (
                "stringToFlags".into(),
                crate::host::capability(crate::registry::SPEC_INTERNAL_FS_STRING_TO_FLAGS),
            ),
            (
                "BigIntStats".into(),
                crate::host::capability(crate::registry::SPEC_FS_STATS),
            ),
            (
                "validateOffsetLengthRead".into(),
                crate::host::capability(
                    crate::registry::SPEC_INTERNAL_FS_VALIDATE_OFFSET_LENGTH_READ,
                ),
            ),
            (
                "validateOffsetLengthWrite".into(),
                crate::host::capability(
                    crate::registry::SPEC_INTERNAL_FS_VALIDATE_OFFSET_LENGTH_WRITE,
                ),
            ),
            (
                "getDirents".into(),
                crate::host::capability(crate::registry::SPEC_INTERNAL_FS_GET_DIRENTS),
            ),
            (
                "getDirent".into(),
                crate::host::capability(crate::registry::SPEC_INTERNAL_FS_GET_DIRENT),
            ),
            (
                "vfsState".into(),
                execute::get_property(
                    &quench_runtime::vm::current_global_object(),
                    "__quenchVfsState",
                ),
            ),
        ])),
        "internal/vfs/stats" => {
            let value = execute::get_property(
                &quench_runtime::vm::current_global_object(),
                "__quenchVfsStatsHelpers",
            );
            matches!(value, Value::Object(_) | Value::ObjectAlias(_)).then_some(value)
        }
        "internal/vfs/router" => internal_vfs_router_module(),
        "internal/vfs/file_handle" => internal_vfs_file_handle_module(),
        "internal/vfs/fd" => internal_vfs_fd_module(),
        "internal/vfs/providers/memory" => internal_vfs_memory_provider_module(),
        "util" | "sys" => {
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
        // Internal util is one Rust-owned namespace. Keep all require paths
        // on the same value so symbols, frozen sentinels, and capabilities
        // cannot diverge between resolver implementations.
        "internal/util" => Some(internal_util_module()),
        "internal/util/inspect" => {
            let util = require(state, &[Value::String("util".into())]).ok()?;
            Some(crate::host::namespace_object_from_pairs(vec![(
                "inspect".into(),
                execute::get_property(&util, "inspect"),
            )]))
        }
        "internal/validators" => Some(crate::host::namespace_object_from_pairs(vec![
            (
                "validateInteger".into(),
                crate::host::capability(crate::registry::SPEC_INTERNAL_VALIDATORS_VALIDATE_INTEGER),
            ),
            (
                "validateOneOf".into(),
                crate::host::capability(crate::registry::SPEC_INTERNAL_VALIDATORS_VALIDATE_ONE_OF),
            ),
            (
                "validatePort".into(),
                crate::host::capability(crate::registry::SPEC_INTERNAL_VALIDATORS_VALIDATE_PORT),
            ),
        ])),
        "internal/url" => Some(host_api::object(vec![(
            "isURL".into(),
            crate::host::capability(crate::registry::SPEC_INTERNAL_URL_IS_URL),
        )])),
        "internal/webstreams/util" => {
            let global = quench_runtime::vm::current_global_object();
            Some(host_api::object(vec![(
                "kState".into(),
                execute::get_property(&global, "__quenchWebStreamsState"),
            )]))
        }
        "internal/crypto/util" => Some(internal_crypto_util_module()),
        "internal/crypto/keys" => Some(internal_crypto_keys_module()),
        "internal/crypto/aes" => Some(crate::host::namespace_object_from_pairs(vec![(
            "aesCipher".into(),
            crate::host::capability(crate::registry::SPEC_INTERNAL_CRYPTO_AES_CIPHER),
        )])),
        "internal/crypto/webidl" => Some(internal_crypto_webidl_module()),
        "internal/crypto/x509" => Some(internal_crypto_x509_module()),
        "internal/crypto/webcrypto" => Some(internal_crypto_webcrypto_module()),
        "internal/test/binding" => Some(crate::host::namespace_object_from_pairs(vec![(
            "internalBinding".to_string(),
            crate::host::capability(crate::registry::SPEC_INTERNAL_BINDING),
        )])),
        "internal/test_runner/utils" => Some(crate::host::namespace_object_from_pairs(vec![
            (
                "convertStringToRegExp".into(),
                crate::host::capability(crate::registry::SPEC_TEST_CONVERT_STRING_TO_REGEXP),
            ),
            (
                "createSeededGenerator".into(),
                crate::host::capability(crate::registry::SPEC_TEST_CREATE_SEEDED_GENERATOR),
            ),
            ("kMaxRandomSeed".into(), Value::Number(4_294_967_295.0)),
        ])),
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
        "internal/errors" => Some(internal_errors_module()),
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
        "stream/iter" => {
            let global = quench_runtime::vm::current_global_object();
            let enabled = ["__quench_argv", "execArgv"].iter().any(|key| {
                let source = if *key == "execArgv" {
                    let process = execute::get_property(&global, "process");
                    execute::get_property(&process, key)
                } else {
                    execute::get_property(&global, key)
                };
                let includes = execute::get_property(&source, "includes");
                matches!(
                    execute::call(
                        &includes,
                        &source,
                        &[Value::String("--experimental-stream-iter".into())]
                    ),
                    Ok(Value::Boolean(true))
                )
            });
            if !enabled {
                return None;
            }
            let global = quench_runtime::vm::current_global_object();
            let stream_iter = execute::get_property(&global, "__quenchRequireStreamIter");
            if matches!(
                stream_iter,
                Value::Function(_) | Value::BoundFunction(_) | Value::Builtin(_)
            ) {
                return execute::call(&stream_iter, &Value::Undefined, &[]).ok();
            }
            Some(crate::host::namespace_object_from_pairs(vec![
                (
                    "text".into(),
                    crate::host::capability(crate::registry::SPEC_STREAM_ITER_TEXT),
                ),
                (
                    "bytes".into(),
                    crate::host::capability(crate::registry::SPEC_STREAM_ITER_BYTES),
                ),
            ]))
        }
        "zlib/iter" => {
            // The iterator surface is installed by the fixture bootstrap when
            // the experimental flag is present. Reuse that one namespace so
            // CJS `require` and the global loader cannot diverge.
            let global = quench_runtime::vm::current_global_object();
            let helper = execute::get_property(&global, "__quenchRequireZlibIter");
            if let Ok(value) = execute::call(&helper, &Value::Undefined, &[]) {
                Some(value)
            } else {
                Some(crate::host::namespace_object_from_pairs(vec![
                    (
                        "compressGzipSync".into(),
                        crate::host::capability(crate::registry::SPEC_ZLIB_ITER_COMPRESS),
                    ),
                    (
                        "decompressGzipSync".into(),
                        crate::host::capability(crate::registry::SPEC_ZLIB_ITER_DECOMPRESS),
                    ),
                ]))
            }
        }
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
        "internal/timers" => Some(host_api::object(vec![
            ("TIMEOUT_MAX".into(), Value::Number(2_147_483_647.0)),
            (
                "setUnrefTimeout".into(),
                crate::host::capability(crate::registry::SPEC_INTERNAL_TIMERS_SET_UNREF_TIMEOUT),
            ),
            (
                "async_context_frame".into(),
                Value::String("Symbol(async_context_frame)\0quench".into()),
            ),
        ])),
        "internal/async_context_frame" => Some(host_api::object(vec![
            ("enabled".into(), Value::Boolean(false)),
            (
                "current".into(),
                crate::host::capability(crate::registry::SPEC_INTERNAL_ASYNC_CONTEXT_FRAME_CURRENT),
            ),
        ])),
        "internal/async_hooks" => Some(host_api::object(vec![
            (
                "symbols".into(),
                host_api::object(vec![
                    (
                        "async_id_symbol".into(),
                        Value::String("Symbol(async_id_symbol)\0quench".into()),
                    ),
                    (
                        "trigger_async_id_symbol".into(),
                        Value::String("Symbol(trigger_async_id_symbol)\0quench".into()),
                    ),
                    (
                        "async_local_storage_context_symbol".into(),
                        Value::String("__nodeAsyncStoresLegacy".into()),
                    ),
                ]),
            ),
            (
                "enabledHooksExist".into(),
                crate::host::capability(
                    crate::registry::SPEC_INTERNAL_ASYNC_HOOKS_ENABLED_HOOKS_EXIST,
                ),
            ),
        ])),
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
            let _ = quench_runtime::execute::set_callable_property(
                &read_stream,
                "prototype",
                quench_runtime::host_api::object(Vec::new()),
            );
            let _ = quench_runtime::execute::set_callable_property(
                &write_stream,
                "prototype",
                quench_runtime::host_api::object(Vec::new()),
            );
            let constructor_parent = quench_runtime::host_api::object(vec![(
                "prototype".into(),
                quench_runtime::host_api::object(Vec::new()),
            )]);
            let _ = quench_runtime::execute::set_prototype_of(&read_stream, &constructor_parent);
            let _ = quench_runtime::execute::set_prototype_of(&write_stream, &constructor_parent);
            Some(crate::host::namespace_object_from_pairs(vec![
                (
                    "isatty".into(),
                    crate::host::capability(crate::registry::SPEC_TTY_ISATTY),
                ),
                ("ReadStream".into(), read_stream),
                ("WriteStream".into(), write_stream),
            ]))
        }
        "fs" | "node:fs" => {
            if let Some(cached) = state.borrow().module_cache.get("fs") {
                return Some(cached.clone());
            }
            let fs = crate::modules::fs::build();
            for name in ["realpath", "realpathSync"] {
                let function = quench_runtime::execute::get_property(&fs, name);
                let _ = quench_runtime::execute::set_callable_property(
                    &function,
                    "native",
                    function.clone(),
                );
            }
            let promises = quench_runtime::execute::get_property(&fs, "promises");
            state
                .borrow_mut()
                .module_cache
                .insert("fs".into(), fs.clone());
            state
                .borrow_mut()
                .module_cache
                .insert("fs/promises".into(), promises);
            Some(fs)
        }
        "fs/promises" | "node:fs/promises" => {
            if let Some(cached) = state.borrow().module_cache.get("fs/promises") {
                return Some(cached.clone());
            }
            let fs = resolve(state, "fs")?;
            let promises = quench_runtime::execute::get_property(&fs, "promises");
            state
                .borrow_mut()
                .module_cache
                .insert("fs/promises".into(), promises.clone());
            Some(promises)
        }
        "vfs" => {
            let global = quench_runtime::vm::current_global_object();
            let module = quench_runtime::execute::get_property(&global, "__nodeVfs");
            (!matches!(module, Value::Undefined)).then_some(module)
        }
        "http" => Some(crate::modules::http::build(state)),
        "readline" => Some(crate::modules::readline::build()),
        "vm" => Some(crate::modules::vm_api::build()),
        "dgram" | "node:dgram" => {
            let global = quench_runtime::vm::current_global_object();
            let module = quench_runtime::execute::get_property(&global, "\0quench:dgram_module");
            if !matches!(module, Value::Undefined) {
                return Some(module);
            }
            None
        }
        "internal/dgram" => {
            let global = quench_runtime::vm::current_global_object();
            let symbol = quench_runtime::execute::get_property(&global, "__quenchDgramStateSymbol");
            Some(host_api::object(vec![("kStateSymbol".into(), symbol)]))
        }
        "https" => {
            let http = crate::modules::http::build(state);
            let http_agent = quench_runtime::execute::get_property(&http, "Agent");
            let http_agent_prototype =
                quench_runtime::execute::get_property(&http_agent, "prototype");
            let agent = quench_runtime::execute::set_property(
                crate::host::capability(crate::registry::SPEC_HTTPS_AGENT),
                "prototype",
                http_agent_prototype,
            );
            let global_agent = crate::modules::http_client::https_agent_construct(
                state,
                &[quench_runtime::host_api::object(vec![(
                    "keepAlive".into(),
                    Value::Boolean(true),
                )])],
            )
            .unwrap_or_else(|_| quench_runtime::execute::get_property(&http, "globalAgent"));
            Some(crate::host::namespace_object_from_pairs(vec![
                // HTTPS shares HTTP's request/response object model.  The
                // transport-specific TLS options remain host metadata on
                // the same server identity until the TLS backend is active.
                (
                    "createServer".to_string(),
                    crate::host::capability(crate::registry::SPEC_HTTPS_CREATE_SERVER),
                ),
                (
                    "Server".to_string(),
                    crate::host::capability(crate::registry::SPEC_HTTPS_CREATE_SERVER),
                ),
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
        "crypto" | "node:crypto" => {
            let global = quench_runtime::vm::current_global_object();
            let hash_proto = host_api::object(Vec::new());
            execute::set_property_in_place(
                &global,
                "\0quench:crypto:hash-prototype",
                hash_proto.clone(),
            );
            let hash_ctor = execute::set_property(
                crate::host::capability(crate::registry::SPEC_CRYPTO_CREATE_HASH),
                "prototype",
                hash_proto,
            );
            let hmac_proto = host_api::object(Vec::new());
            execute::set_property_in_place(
                &global,
                "\0quench:crypto:hmac-prototype",
                hmac_proto.clone(),
            );
            let hmac_ctor = execute::set_property(
                crate::host::capability(crate::registry::SPEC_CRYPTO_CREATE_HMAC),
                "prototype",
                hmac_proto,
            );
            let sign_proto = host_api::object(Vec::new());
            let verify_proto = host_api::object(Vec::new());
            execute::set_property_in_place(
                &global,
                "\0quench:crypto:sign-prototype",
                sign_proto.clone(),
            );
            execute::set_property_in_place(
                &global,
                "\0quench:crypto:verify-prototype",
                verify_proto.clone(),
            );
            let sign_ctor = execute::set_property(
                crate::host::capability(crate::registry::SPEC_CRYPTO_CREATE_SIGN),
                "prototype",
                sign_proto,
            );
            let verify_ctor = execute::set_property(
                crate::host::capability(crate::registry::SPEC_CRYPTO_CREATE_VERIFY),
                "prototype",
                verify_proto,
            );
            let dh_proto = crate::modules::crypto_dh::prototype();
            execute::set_property_in_place(
                &global,
                "\0quench:crypto:dh-prototype",
                dh_proto.clone(),
            );
            let dh_ctor = execute::set_property(
                crate::host::capability(crate::registry::SPEC_CRYPTO_CREATE_DIFFIE_HELLMAN),
                "prototype",
                dh_proto.clone(),
            );
            execute::set_property_in_place(
                &global,
                "\0quench:crypto:dh-constructor",
                dh_ctor.clone(),
            );
            let dh_group_ctor = execute::set_property(
                crate::host::capability(crate::registry::SPEC_CRYPTO_CREATE_DIFFIE_HELLMAN_GROUP),
                "prototype",
                dh_proto,
            );
            execute::set_property_in_place(
                &global,
                "\0quench:crypto:dh-group-constructor",
                dh_group_ctor.clone(),
            );
            let ecdh_proto = host_api::object(Vec::new());
            execute::set_property_in_place(
                &global,
                "\0quench:crypto:ecdh-prototype",
                ecdh_proto.clone(),
            );
            let ecdh_ctor = execute::set_property(
                crate::host::capability(crate::registry::SPEC_CRYPTO_CREATE_ECDH),
                "prototype",
                ecdh_proto,
            );
            let ecdh_ctor = execute::set_property(
                ecdh_ctor,
                "convertKey",
                crate::host::capability(crate::registry::SPEC_CRYPTO_ECDH_CONVERT_KEY),
            );
            let cipher_proto = host_api::object(Vec::new());
            execute::set_property_in_place(
                &global,
                "\0quench:crypto:cipher-prototype",
                cipher_proto.clone(),
            );
            execute::set_property_in_place(
                &global,
                "\0quench:crypto:decipher-prototype",
                cipher_proto.clone(),
            );
            let cipher_ctor = execute::set_property(
                crate::host::capability(crate::registry::SPEC_CRYPTO_CREATE_CIPHERIV),
                "prototype",
                cipher_proto.clone(),
            );
            let decipher_ctor = execute::set_property(
                crate::host::capability(crate::registry::SPEC_CRYPTO_CREATE_DECIPHERIV),
                "prototype",
                cipher_proto,
            );
            let key_object_ctor = execute::set_property(
                crate::host::capability(crate::registry::SPEC_CRYPTO_KEY_OBJECT_CONSTRUCTOR),
                "from",
                crate::host::capability(crate::registry::SPEC_CRYPTO_KEY_OBJECT_FROM),
            );
            let key_object_ctor = execute::set_property(
                key_object_ctor,
                "prototype",
                crate::modules::crypto::key_object_prototypes().0,
            );
            let certificate_ctor = execute::set_property(
                crate::host::capability(crate::registry::SPEC_CRYPTO_CERTIFICATE_CONSTRUCTOR),
                "prototype",
                crate::modules::crypto::certificate_prototype(),
            );
            let certificate_ctor = execute::set_property(
                certificate_ctor,
                "verifySpkac",
                crate::host::capability(crate::registry::SPEC_CRYPTO_CERTIFICATE_VERIFY_SPKAC),
            );
            let certificate_ctor = execute::set_property(
                certificate_ctor,
                "exportPublicKey",
                crate::host::capability(crate::registry::SPEC_CRYPTO_CERTIFICATE_EXPORT_PUBLIC_KEY),
            );
            let certificate_ctor = execute::set_property(
                certificate_ctor,
                "exportChallenge",
                crate::host::capability(crate::registry::SPEC_CRYPTO_CERTIFICATE_EXPORT_CHALLENGE),
            );
            let namespace = crate::host::namespace_object_from_pairs(vec![
                (
                    "createSecretKey".into(),
                    crate::host::capability(crate::registry::SPEC_CRYPTO_CREATE_SECRET_KEY),
                ),
                (
                    "createPrivateKey".into(),
                    crate::host::capability(crate::registry::SPEC_CRYPTO_CREATE_PRIVATE_KEY),
                ),
                (
                    "createPublicKey".into(),
                    crate::host::capability(crate::registry::SPEC_CRYPTO_CREATE_PUBLIC_KEY),
                ),
                (
                    "publicEncrypt".into(),
                    crate::host::capability(crate::registry::SPEC_CRYPTO_PUBLIC_ENCRYPT),
                ),
                (
                    "privateDecrypt".into(),
                    crate::host::capability(crate::registry::SPEC_CRYPTO_PRIVATE_DECRYPT),
                ),
                (
                    "publicDecrypt".into(),
                    crate::host::capability(crate::registry::SPEC_CRYPTO_PUBLIC_DECRYPT),
                ),
                (
                    "privateEncrypt".into(),
                    crate::host::capability(crate::registry::SPEC_CRYPTO_PRIVATE_ENCRYPT),
                ),
                (
                    "generateKeyPairSync".into(),
                    crate::host::capability(crate::registry::SPEC_CRYPTO_GENERATE_KEY_PAIR_SYNC),
                ),
                (
                    "generateKeyPair".into(),
                    crate::host::capability(crate::registry::SPEC_CRYPTO_GENERATE_KEY_PAIR),
                ),
                (
                    "generateKeySync".into(),
                    crate::host::capability(crate::registry::SPEC_CRYPTO_GENERATE_KEY_SYNC),
                ),
                (
                    "generateKey".into(),
                    crate::host::capability(crate::registry::SPEC_CRYPTO_GENERATE_KEY),
                ),
                (
                    "argon2".into(),
                    crate::host::capability(crate::registry::SPEC_CRYPTO_ARGON2),
                ),
                (
                    "hkdfSync".into(),
                    crate::host::capability(crate::registry::SPEC_CRYPTO_HKDF_SYNC),
                ),
                (
                    "hkdf".into(),
                    crate::host::capability(crate::registry::SPEC_CRYPTO_HKDF),
                ),
                ("KeyObject".into(), key_object_ctor),
                ("Certificate".into(), certificate_ctor),
                (
                    "X509Certificate".into(),
                    crate::host::capability(crate::registry::SPEC_CRYPTO_X509_CONSTRUCTOR),
                ),
                (
                    "createHash".into(),
                    crate::host::capability(crate::registry::SPEC_CRYPTO_CREATE_HASH),
                ),
                ("Hash".into(), hash_ctor),
                (
                    "hash".into(),
                    crate::host::capability(crate::registry::SPEC_CRYPTO_HASH),
                ),
                (
                    "createHmac".into(),
                    crate::host::capability(crate::registry::SPEC_CRYPTO_CREATE_HMAC),
                ),
                ("Hmac".into(), hmac_ctor),
                (
                    "createSign".into(),
                    crate::host::capability(crate::registry::SPEC_CRYPTO_CREATE_SIGN),
                ),
                (
                    "createVerify".into(),
                    crate::host::capability(crate::registry::SPEC_CRYPTO_CREATE_VERIFY),
                ),
                ("Sign".into(), sign_ctor),
                ("Verify".into(), verify_ctor),
                (
                    "sign".into(),
                    crate::host::capability(crate::registry::SPEC_CRYPTO_SIGN),
                ),
                (
                    "verify".into(),
                    crate::host::capability(crate::registry::SPEC_CRYPTO_VERIFY),
                ),
                ("DiffieHellman".into(), dh_ctor),
                ("DiffieHellmanGroup".into(), dh_group_ctor),
                (
                    "createDiffieHellman".into(),
                    crate::host::capability(crate::registry::SPEC_CRYPTO_CREATE_DIFFIE_HELLMAN),
                ),
                (
                    "createDiffieHellmanGroup".into(),
                    crate::host::capability(
                        crate::registry::SPEC_CRYPTO_CREATE_DIFFIE_HELLMAN_GROUP,
                    ),
                ),
                (
                    "getDiffieHellman".into(),
                    crate::host::capability(crate::registry::SPEC_CRYPTO_GET_DIFFIE_HELLMAN),
                ),
                ("ECDH".into(), ecdh_ctor),
                (
                    "createECDH".into(),
                    crate::host::capability(crate::registry::SPEC_CRYPTO_CREATE_ECDH),
                ),
                (
                    "getCurves".into(),
                    crate::host::capability(crate::registry::SPEC_CRYPTO_GET_CURVES),
                ),
                (
                    "diffieHellman".into(),
                    crate::host::capability(crate::registry::SPEC_CRYPTO_DIFFIE_HELLMAN),
                ),
                (
                    "constants".into(),
                    host_api::object(vec![
                        ("RSA_PKCS1_PADDING".into(), Value::Number(1.0)),
                        ("RSA_SSLV23_PADDING".into(), Value::Number(2.0)),
                        ("RSA_NO_PADDING".into(), Value::Number(3.0)),
                        ("RSA_PKCS1_OAEP_PADDING".into(), Value::Number(4.0)),
                        ("RSA_X931_PADDING".into(), Value::Number(5.0)),
                        ("RSA_PKCS1_PSS_PADDING".into(), Value::Number(6.0)),
                        ("RSA_PSS_SALTLEN_DIGEST".into(), Value::Number(-1.0)),
                        ("RSA_PSS_SALTLEN_AUTO".into(), Value::Number(-2.0)),
                        ("RSA_PSS_SALTLEN_MAX_SIGN".into(), Value::Number(-2.0)),
                        ("DH_CHECK_P_NOT_PRIME".into(), Value::Number(1.0)),
                        ("DH_CHECK_P_NOT_SAFE_PRIME".into(), Value::Number(2.0)),
                        ("DH_UNABLE_TO_CHECK_GENERATOR".into(), Value::Number(4.0)),
                        ("DH_NOT_SUITABLE_GENERATOR".into(), Value::Number(8.0)),
                    ]),
                ),
                (
                    "getHashes".into(),
                    crate::host::capability(crate::registry::SPEC_CRYPTO_GET_HASHES),
                ),
                (
                    "getFips".into(),
                    crate::host::capability(crate::registry::SPEC_CRYPTO_GET_FIPS),
                ),
                (
                    "setFips".into(),
                    crate::host::capability(crate::registry::SPEC_CRYPTO_SET_FIPS),
                ),
                (
                    "setEngine".into(),
                    crate::host::capability(crate::registry::SPEC_CRYPTO_SET_ENGINE),
                ),
                (
                    "secureHeapUsed".into(),
                    crate::host::capability(crate::registry::SPEC_CRYPTO_SECURE_HEAP_USED),
                ),
                (
                    "encapsulate".into(),
                    crate::host::capability(crate::registry::SPEC_CRYPTO_ENCAPSULATE),
                ),
                (
                    "decapsulate".into(),
                    crate::host::capability(crate::registry::SPEC_CRYPTO_DECAPSULATE),
                ),
                (
                    "checkPrimeSync".into(),
                    crate::host::capability(crate::registry::SPEC_CRYPTO_CHECK_PRIME_SYNC),
                ),
                (
                    "checkPrime".into(),
                    crate::host::capability(crate::registry::SPEC_CRYPTO_CHECK_PRIME),
                ),
                (
                    "generatePrimeSync".into(),
                    crate::host::capability(crate::registry::SPEC_CRYPTO_GENERATE_PRIME_SYNC),
                ),
                (
                    "generatePrime".into(),
                    crate::host::capability(crate::registry::SPEC_CRYPTO_GENERATE_PRIME),
                ),
                (
                    "randomBytes".into(),
                    crate::host::capability(crate::registry::SPEC_CRYPTO_RANDOM_BYTES),
                ),
                (
                    "randomFillSync".into(),
                    crate::host::capability(crate::registry::SPEC_CRYPTO_RANDOM_FILL_SYNC),
                ),
                (
                    "randomFill".into(),
                    crate::host::capability(crate::registry::SPEC_CRYPTO_RANDOM_FILL),
                ),
                (
                    "randomInt".into(),
                    crate::host::capability(crate::registry::SPEC_CRYPTO_RANDOM_INT),
                ),
                (
                    "randomUUIDv7".into(),
                    crate::host::capability(crate::registry::SPEC_CRYPTO_RANDOM_UUIDV7),
                ),
                (
                    "randomUUID".into(),
                    crate::host::capability(crate::registry::SPEC_CRYPTO_RANDOM_UUID),
                ),
                (
                    "pseudoRandomBytes".into(),
                    crate::host::capability(crate::registry::SPEC_CRYPTO_RANDOM_BYTES),
                ),
                (
                    "prng".into(),
                    crate::host::capability(crate::registry::SPEC_CRYPTO_RANDOM_BYTES),
                ),
                (
                    "rng".into(),
                    crate::host::capability(crate::registry::SPEC_CRYPTO_RANDOM_BYTES),
                ),
                (
                    "pbkdf2Sync".into(),
                    crate::host::capability(crate::registry::SPEC_CRYPTO_PBKDF2_SYNC),
                ),
                (
                    "pbkdf2".into(),
                    crate::host::capability(crate::registry::SPEC_CRYPTO_PBKDF2),
                ),
                (
                    "scryptSync".into(),
                    crate::host::capability(crate::registry::SPEC_CRYPTO_SCRYPT_SYNC),
                ),
                (
                    "scrypt".into(),
                    crate::host::capability(crate::registry::SPEC_CRYPTO_SCRYPT),
                ),
                (
                    "createCipheriv".into(),
                    crate::host::capability(crate::registry::SPEC_CRYPTO_CREATE_CIPHERIV),
                ),
                (
                    "createDecipheriv".into(),
                    crate::host::capability(crate::registry::SPEC_CRYPTO_CREATE_DECIPHERIV),
                ),
                ("Cipheriv".into(), cipher_ctor),
                ("Decipheriv".into(), decipher_ctor),
                (
                    "getCiphers".into(),
                    crate::host::capability(crate::registry::SPEC_CRYPTO_GET_CIPHERS),
                ),
                (
                    "getCipherInfo".into(),
                    crate::host::capability(crate::registry::SPEC_CRYPTO_GET_CIPHER_INFO),
                ),
                ("webcrypto".into(), execute::get_property(&global, "crypto")),
            ]);
            for name in ["pseudoRandomBytes", "prng", "rng"] {
                let descriptor =
                    host_api::object(vec![("enumerable".into(), Value::Boolean(false))]);
                let _ = execute::define_property(namespace.clone(), name, descriptor);
            }
            Some(namespace)
        }
        "zlib" => Some(crate::modules::zlib::build()),
        "tls" => Some(crate::modules::tls::build(state)),
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
        "v8" => crate::modules::compat_extra::v8(state).ok(),
        "trace_events" => Some(crate::modules::trace_events::build()),
        "repl" => Some(crate::modules::repl::build()),
        "wasi" => Some(crate::modules::wasi::build()),
        "worker_threads" => crate::modules::compat_extra::worker_threads(state).ok(),
        "sea" => {
            let factory = eval_module_factory(
                "() => ({ isSea: false, getAsset() { const error = new Error('Cannot use require(\\\"sea\\\") outside a single executable application'); error.code = 'ERR_NOT_SUPPORTED'; throw error; } })",
            )?;
            execute::call(&factory, &Value::Undefined, &[]).ok()
        }
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
            let _ = attach(&nested_fn, "skip", skip_fn);
            for alias in ["describe", "suite"] {
                let _ = attach(&test_fn, alias, nested_fn.clone());
            }
            let _ = attach(&test_fn, "run", test_fn.clone());
            let shorthand = |mode: &str| {
                quench_runtime::host_api::bound_capability_with_arguments(
                    quench_runtime::ops::HostCapabilityRef {
                        realm: quench_runtime::ops::RealmId::ROOT,
                        kind: quench_runtime::ops::HostCapabilityKind::Custom(
                            crate::registry::SPEC_TEST_SHORTHAND.cap,
                        ),
                    },
                    vec![Value::String(mode.into())],
                )
            };
            let _ = attach(&test_fn, "only", shorthand("only"));
            let _ = attach(&test_fn, "todo", shorthand("todo"));
            let _ = attach(&nested_fn, "only", shorthand("only:nested"));
            let _ = attach(&nested_fn, "todo", shorthand("todo:nested"));
            let _ = attach(
                &test_fn,
                "getTestContext",
                crate::host::capability(crate::registry::SPEC_TEST_GET_CONTEXT),
            );
            let _ = attach(
                &test_fn,
                "before",
                crate::host::capability(crate::registry::SPEC_TEST_BEFORE),
            );
            let _ = attach(
                &test_fn,
                "after",
                crate::host::capability(crate::registry::SPEC_TEST_AFTER),
            );
            let _ = attach(
                &test_fn,
                "beforeEach",
                crate::host::capability(crate::registry::SPEC_TEST_BEFORE_EACH),
            );
            let _ = attach(
                &test_fn,
                "afterEach",
                crate::host::capability(crate::registry::SPEC_TEST_AFTER_EACH),
            );
            let _ = attach(
                &test_fn,
                "assert",
                quench_runtime::host_api::object(vec![(
                    "register".into(),
                    crate::host::capability(crate::registry::SPEC_TEST_ASSERT_REGISTER),
                )]),
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
                        (
                            "runAll".to_string(),
                            crate::host::capability(crate::registry::SPEC_TEST_MOCK_TIMERS_RUN_ALL),
                        ),
                        (
                            "Symbol.dispose".to_string(),
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
        "stream/promises" => stream_promises_module(state),
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
            if let Ok(mut host) = state.try_borrow_mut() {
                host.child_process_prototype = Some(prototype);
            }
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

/// Minimal HTTP/2 namespace for API consumers that only need the public
/// callable identity (for example `util.promisify(http2.connect)`).  The
/// protocol implementation remains owned by the HTTP host module; keeping the
/// namespace construction here avoids manufacturing a second loader path.
pub(crate) fn http2_module_value() -> Value {
    let connect = quench_runtime::host_api::bound_builtin(
        quench_runtime::ops::Builtin::Object,
        Value::Undefined,
    );
    let connect = execute::set_property(connect, "name", Value::String("connect".into()));
    crate::host::namespace_object_from_pairs(vec![
        ("connect".into(), connect),
        (
            "sensitiveHeaders".into(),
            crate::modules::http2_util::sensitive_headers(),
        ),
    ])
}

fn internal_crypto_util_module() -> Value {
    let digest = crate::host::namespace_object_from_pairs(
        [
            "SHA-1",
            "SHA-224",
            "SHA-256",
            "SHA-384",
            "SHA-512",
            "SHA3-224",
            "SHA3-256",
            "SHA3-384",
            "SHA3-512",
            "MD5",
            "RIPEMD160",
        ]
        .into_iter()
        .map(|name| (name.to_string(), Value::Boolean(true)))
        .collect(),
    );
    let import_key = crate::host::namespace_object_from_pairs(
        [
            "AES-KW",
            "ECDH",
            "ECDSA",
            "Ed25519",
            "HMAC",
            "RSA-OAEP",
            "RSA-PSS",
            "RSASSA-PKCS1-v1_5",
            "X25519",
        ]
        .into_iter()
        .map(|name| (name.to_string(), Value::Boolean(true)))
        .collect(),
    );
    crate::host::namespace_object_from_pairs(vec![
        (
            "getOpenSSLSecLevel".into(),
            crate::host::capability(crate::registry::SPEC_INTERNAL_CRYPTO_GET_OPENSSL_SEC_LEVEL),
        ),
        (
            "bigIntArrayToUnsignedInt".into(),
            crate::host::capability(
                crate::registry::SPEC_INTERNAL_CRYPTO_BIGINT_ARRAY_TO_UNSIGNED_INT,
            ),
        ),
        (
            "bigIntArrayToUnsignedBigInt".into(),
            crate::host::capability(
                crate::registry::SPEC_INTERNAL_CRYPTO_BIGINT_ARRAY_TO_UNSIGNED_BIGINT,
            ),
        ),
        (
            "normalizeAlgorithm".into(),
            crate::host::capability(crate::registry::SPEC_INTERNAL_CRYPTO_NORMALIZE_ALGORITHM),
        ),
        (
            "validateKeyOps".into(),
            crate::host::capability(crate::registry::SPEC_INTERNAL_CRYPTO_VALIDATE_KEY_OPS),
        ),
        (
            "getUsagesMask".into(),
            crate::host::capability(crate::registry::SPEC_INTERNAL_CRYPTO_GET_USAGES_MASK),
        ),
        (
            "kSupportedAlgorithms".into(),
            crate::host::namespace_object_from_pairs(vec![
                ("digest".into(), digest),
                ("importKey".into(), import_key),
            ]),
        ),
        (
            "kHandle".into(),
            Value::String("Symbol.kHandle\0crypto".into()),
        ),
    ])
}

fn internal_crypto_keys_module() -> Value {
    crate::host::namespace_object_from_pairs(vec![(
        "getCryptoKeyHandle".into(),
        crate::host::capability(crate::registry::SPEC_INTERNAL_CRYPTO_KEY_HANDLE),
    )])
}

fn internal_crypto_webidl_module() -> Value {
    let mut converters = vec![
        (
            "boolean".into(),
            crate::host::capability(crate::registry::SPEC_INTERNAL_CRYPTO_WEBIDL_BOOLEAN),
        ),
        (
            "octet".into(),
            crate::host::capability(crate::registry::SPEC_INTERNAL_CRYPTO_WEBIDL_OCTET),
        ),
        (
            "unsigned short".into(),
            crate::host::capability(crate::registry::SPEC_INTERNAL_CRYPTO_WEBIDL_UNSIGNED_SHORT),
        ),
        (
            "unsigned long".into(),
            crate::host::capability(crate::registry::SPEC_INTERNAL_CRYPTO_WEBIDL_UNSIGNED_LONG),
        ),
        (
            "DOMString".into(),
            crate::host::capability(crate::registry::SPEC_INTERNAL_CRYPTO_WEBIDL_DOM_STRING),
        ),
        (
            "object".into(),
            crate::host::capability(crate::registry::SPEC_INTERNAL_CRYPTO_WEBIDL_OBJECT),
        ),
        (
            "Uint8Array".into(),
            crate::host::capability(crate::registry::SPEC_INTERNAL_CRYPTO_WEBIDL_UINT8_ARRAY),
        ),
        (
            "BigInteger".into(),
            crate::host::capability(crate::registry::SPEC_INTERNAL_CRYPTO_WEBIDL_BIG_INTEGER),
        ),
        (
            "BufferSource".into(),
            crate::host::capability(crate::registry::SPEC_INTERNAL_CRYPTO_WEBIDL_BUFFER_SOURCE),
        ),
        (
            "CryptoKey".into(),
            crate::host::capability(crate::registry::SPEC_INTERNAL_CRYPTO_WEBIDL_CRYPTO_KEY),
        ),
        (
            "AlgorithmIdentifier".into(),
            crate::host::capability(
                crate::registry::SPEC_INTERNAL_CRYPTO_WEBIDL_ALGORITHM_IDENTIFIER,
            ),
        ),
        (
            "KeyFormat".into(),
            crate::host::capability(crate::registry::SPEC_INTERNAL_CRYPTO_WEBIDL_KEY_FORMAT),
        ),
        (
            "KeyUsage".into(),
            crate::host::capability(crate::registry::SPEC_INTERNAL_CRYPTO_WEBIDL_KEY_USAGE),
        ),
        (
            "JsonWebKey".into(),
            crate::host::capability(crate::registry::SPEC_INTERNAL_CRYPTO_WEBIDL_JSON_WEB_KEY),
        ),
        (
            "Algorithm".into(),
            crate::host::capability(crate::registry::SPEC_INTERNAL_CRYPTO_WEBIDL_ALGORITHM),
        ),
        (
            "RsaOaepParams".into(),
            crate::host::capability(crate::registry::SPEC_INTERNAL_CRYPTO_WEBIDL_RSA_OAEP),
        ),
        (
            "EcKeyImportParams".into(),
            crate::host::capability(crate::registry::SPEC_INTERNAL_CRYPTO_WEBIDL_EC_IMPORT),
        ),
        (
            "EcKeyGenParams".into(),
            crate::host::capability(crate::registry::SPEC_INTERNAL_CRYPTO_WEBIDL_EC_GEN),
        ),
        (
            "EcdsaParams".into(),
            crate::host::capability(crate::registry::SPEC_INTERNAL_CRYPTO_WEBIDL_ECDSA),
        ),
        (
            "HmacKeyGenParams".into(),
            crate::host::capability(crate::registry::SPEC_INTERNAL_CRYPTO_WEBIDL_HMAC_KEYGEN),
        ),
        (
            "HmacImportParams".into(),
            crate::host::capability(crate::registry::SPEC_INTERNAL_CRYPTO_WEBIDL_HMAC_IMPORT),
        ),
        (
            "AesKeyGenParams".into(),
            crate::host::capability(crate::registry::SPEC_INTERNAL_CRYPTO_WEBIDL_AES_KEYGEN),
        ),
        (
            "AesDerivedKeyParams".into(),
            crate::host::capability(crate::registry::SPEC_INTERNAL_CRYPTO_WEBIDL_AES_DERIVED),
        ),
        (
            "HkdfParams".into(),
            crate::host::capability(crate::registry::SPEC_INTERNAL_CRYPTO_WEBIDL_HKDF),
        ),
        (
            "Pbkdf2Params".into(),
            crate::host::capability(crate::registry::SPEC_INTERNAL_CRYPTO_WEBIDL_PBKDF2),
        ),
        (
            "Argon2Params".into(),
            crate::host::capability(crate::registry::SPEC_INTERNAL_CRYPTO_WEBIDL_ARGON2),
        ),
        (
            "AesCbcParams".into(),
            crate::host::capability(crate::registry::SPEC_INTERNAL_CRYPTO_WEBIDL_AES_CBC),
        ),
        (
            "AeadParams".into(),
            crate::host::capability(crate::registry::SPEC_INTERNAL_CRYPTO_WEBIDL_AEAD),
        ),
        (
            "AesCtrParams".into(),
            crate::host::capability(crate::registry::SPEC_INTERNAL_CRYPTO_WEBIDL_AES_CTR),
        ),
        (
            "EcdhKeyDeriveParams".into(),
            crate::host::capability(crate::registry::SPEC_INTERNAL_CRYPTO_WEBIDL_ECDH),
        ),
    ];
    let names = [
        "ContextParams",
        "RsaKeyGenParams",
        "RsaHashedImportParams",
        "RsaHashedKeyGenParams",
        "RsaPssParams",
        "CShakeParams",
        "KmacKeyGenParams",
        "KmacImportParams",
        "KmacParams",
        "KangarooTwelveParams",
        "TurboShakeParams",
    ];
    converters.extend(names.into_iter().map(|name| {
        (
            name.into(),
            crate::host::capability(crate::registry::SPEC_INTERNAL_CRYPTO_WEBIDL_DICTIONARY),
        )
    }));
    let required =
        crate::host::capability(crate::registry::SPEC_INTERNAL_CRYPTO_WEBIDL_REQUIRED_ARGUMENTS);
    crate::host::namespace_object_from_pairs(vec![
        ("requiredArguments".into(), required),
        (
            "converters".into(),
            crate::host::namespace_object_from_pairs(converters),
        ),
    ])
}

fn internal_crypto_x509_module() -> Value {
    crate::host::namespace_object_from_pairs(vec![(
        "isX509Certificate".into(),
        crate::host::capability(crate::registry::SPEC_INTERNAL_CRYPTO_IS_X509_CERTIFICATE),
    )])
}

fn internal_crypto_webcrypto_module() -> Value {
    let global = quench_runtime::vm::current_global_object();
    crate::host::namespace_object_from_pairs(
        ["Crypto", "CryptoKey", "SubtleCrypto"]
            .into_iter()
            .map(|name| {
                (
                    name.to_string(),
                    quench_runtime::execute::get_property(&global, name),
                )
            })
            .collect(),
    )
}

fn value_to_string(value: &Value) -> String {
    quench_runtime::to_string(value).unwrap_or_default()
}
