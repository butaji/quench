//! Host trait implementation. One `NodeHost` impl, one dispatch.
//!
//! Builtins return `Value::Object` they own (plain Rust objects
//! exposed through the runtime's ordinary object semantics). The
//! host never re-enters the VM for state — every state lives in
//! the Rust envelope.

use std::cell::RefCell;
use std::collections::HashSet;
use std::rc::Rc;

use quench_runtime::execute::VmError;
use quench_runtime::host_api;
use quench_runtime::ops::{HostCapabilityKind, HostCapabilityRef, RealmId};
use quench_runtime::value::Value;
use quench_runtime::vm::{Host, OutputSink, VmContext};

use crate::registry::{CapId, NodeSpec};

pub fn scheduler_capability(kind: u16) -> Value {
    host_api::capability_function(HostCapabilityRef {
        realm: RealmId::ROOT,
        kind: HostCapabilityKind::Custom(kind),
    })
}

/// Canonical process.cpuUsage host capability used by the Rust process module.
pub fn process_cpu_usage_capability() -> Value {
    capability(crate::registry::SPEC_PROCESS_CPU_USAGE)
}

pub fn process_uptime_capability() -> Value {
    capability(crate::registry::SPEC_PROCESS_UPTIME)
}

pub struct NodeHost {
    state: Rc<RefCell<HostState>>,
}

pub struct HostState {
    pub async_hooks: crate::modules::async_hooks::AsyncHooksState,
    pub timers: crate::modules::timers::TimerRegistry,
    pub event_loop: crate::modules::event_loop::EventLoop,
    pub process: crate::modules::process::ProcessState,
    pub fs: crate::modules::fs::FsState,
    pub net: crate::modules::net::NetState,
    pub http: crate::modules::http::HttpState,
    pub emitters: crate::modules::emitter::EmitterRegistry,
    pub targets: crate::modules::event_target::TargetRegistry,
    pub diagnostics: crate::modules::diagnostics_channel::DiagnosticsState,
    pub domain: crate::modules::domain::DomainState,
    pub cluster: crate::modules::cluster::ClusterState,
    pub stopped_events: HashSet<u64>,
    pub dispatching_events: HashSet<u64>,
    pub output: Option<OutputSink>,
    pub realm: RealmId,
    /// Directory stack for the CJS loader: top is the requiring module's dir.
    pub dir_stack: Vec<String>,
    /// CJS module cache keyed by canonical file path.
    pub module_cache: std::collections::HashMap<String, Value>,
    /// Module record handed to `__quench_cjs_wrap__` for the file
    /// currently being loaded by `require`.
    pub pending_module: Option<PendingModule>,
    /// CJS filename stack used by module lifecycle instrumentation.
    pub module_stack: Vec<String>,
    /// Thrown value stashed by `pump::handle_uncaught`, dispatched by
    /// the `__quench_uncaught__` capability inside an active frame.
    pub pending_uncaught: Option<Value>,
    /// Paths of internal FileHandles whose finalizer must report an
    /// `ERR_INVALID_STATE` at the next explicit GC boundary.
    pub pending_filehandle_gc: Vec<String>,
    /// Shared `URL` class pair (constructor, prototype), built on first use
    /// so `instanceof URL` has one canonical prototype per realm.
    pub url_class: Option<(Value, Value)>,
    /// Strong roots for live `blob:nodedata:` registrations.
    pub blob_urls: std::collections::HashMap<String, Value>,
    pub next_blob_url: u64,
    /// `require('stream')` module value, evaluated once from the
    /// embedded JS prelude (`modules/stream_prelude.js`).
    pub stream_module: Option<Value>,
    /// Original compose evaluator retained behind the Rust static boundary.
    pub stream_compose_impl: Option<Value>,
    /// Original web-aware pipeline evaluator retained behind the Rust
    /// boundary; native stream-only pipelines use the Rust state machine.
    pub stream_pipeline_impl: Option<Value>,
    /// `require('stream/consumers')` value, evaluated once per realm.
    pub stream_consumers_module: Option<Value>,
    /// Canonical `require("util")` module for this realm.
    pub util_module: Option<Value>,
    /// Canonical `require("console")` module and global console identity.
    pub console_module: Option<Value>,
    /// Canonical `require("process")` module and global process identity.
    pub process_module: Option<Value>,
    /// Canonical `require("module")` namespace for this realm.
    pub module_api: Option<Value>,
    /// Canonical `require.extensions` table for this realm.
    pub module_extensions: Option<Value>,
    pub string_decoder_aliases: std::collections::HashMap<u64, u64>,
    pub string_decoder_pending: std::collections::HashMap<u64, Vec<u8>>,
    pub string_decoder_encoding: std::collections::HashMap<u64, String>,
    pub string_decoder_next_id: u64,
    /// Canonical `internalBinding("os")` object for this realm.
    pub os_binding: Option<Value>,
    /// Canonical `internalBinding("cares_wrap")` object for this realm.
    pub cares_binding: Option<Value>,
    /// Canonical `internalBinding("tcp_wrap")` object for this realm.
    pub tcp_binding: Option<Value>,
    /// Composite AbortSignals keyed by each source target identity.
    pub abort_composites: std::collections::HashMap<u64, Vec<quench_runtime::value::WeakObject>>,
    /// Weak handles for source signals whose dependent-set metadata is
    /// updated after host-side GC pruning.
    pub abort_signal_refs: std::collections::HashMap<u64, quench_runtime::value::WeakObject>,
    /// Strong roots for host-created identity-bearing objects exposed as aliases.
    pub identity_roots: Vec<Value>,
    /// Canonical child-process prototype kept out of the JavaScript global.
    pub child_process_prototype: Option<Value>,
    /// One canonical source-to-stdin edge for in-process stdio pipelines.
    pub child_pipes: std::collections::HashMap<u64, Value>,
}

/// Host-side handoff record for one in-flight CJS module load.
pub struct PendingModule {
    pub module: Value,
    pub filename: String,
    pub dirname: String,
}

impl NodeHost {
    pub fn new(realm: RealmId, argv: Vec<String>) -> Self {
        // The Node test common/tmpdir helper exposes a per-process host path.
        // Materialize that parent at host construction so fixtures can create
        // files there even when the helper's JS-side refresh hook is absent
        // from a reduced bootstrap realm.
        let tmp = std::path::PathBuf::from(format!(
            "/tmp/quench-node-{}",
            std::process::id()
        ));
        let _ = std::fs::create_dir_all(tmp);
        let state = HostState {
            async_hooks: crate::modules::async_hooks::AsyncHooksState::new(),
            timers: crate::modules::timers::TimerRegistry::new(),
            event_loop: crate::modules::event_loop::EventLoop::new(),
            process: crate::modules::process::ProcessState::new(argv),
            fs: crate::modules::fs::FsState::new(),
            net: crate::modules::net::NetState::new(),
            http: crate::modules::http::HttpState::new(),
            emitters: crate::modules::emitter::EmitterRegistry::new(),
            targets: crate::modules::event_target::TargetRegistry::new(),
            diagnostics: crate::modules::diagnostics_channel::DiagnosticsState::new(),
            domain: crate::modules::domain::DomainState::new(),
            cluster: crate::modules::cluster::ClusterState::new(),
            stopped_events: HashSet::new(),
            dispatching_events: HashSet::new(),
            output: None,
            realm,
            dir_stack: Vec::new(),
            module_cache: std::collections::HashMap::new(),
            pending_module: None,
            module_stack: Vec::new(),
            pending_uncaught: None,
            pending_filehandle_gc: Vec::new(),
            url_class: None,
            blob_urls: std::collections::HashMap::new(),
            next_blob_url: 1,
            stream_module: None,
            stream_compose_impl: None,
            stream_pipeline_impl: None,
            stream_consumers_module: None,
            util_module: None,
            console_module: None,
            process_module: None,
            module_api: None,
            module_extensions: None,
            string_decoder_aliases: std::collections::HashMap::new(),
            string_decoder_pending: std::collections::HashMap::new(),
            string_decoder_encoding: std::collections::HashMap::new(),
            string_decoder_next_id: 1,
            os_binding: None,
            cares_binding: None,
            tcp_binding: None,
            abort_composites: std::collections::HashMap::new(),
            abort_signal_refs: std::collections::HashMap::new(),
            identity_roots: Vec::new(),
            child_process_prototype: None,
            child_pipes: std::collections::HashMap::new(),
        };
        Self {
            state: Rc::new(RefCell::new(state)),
        }
    }

    pub fn with_output_sink(self, sink: OutputSink) -> Self {
        self.state.borrow_mut().output = Some(sink);
        self
    }

    pub fn state(&self) -> Rc<RefCell<HostState>> {
        self.state.clone()
    }

    /// Seed the CJS loader with the main script's directory.
    pub fn set_main_dir(&self, dir: String) {
        self.state.borrow_mut().dir_stack = vec![dir];
    }

    /// Exit code recorded by `process.exit`, if any.
    pub fn exit_code(&self) -> Option<i32> {
        self.state.borrow().process.exit_code
    }
}

impl Host for NodeHost {
    fn call(
        &self,
        capability: HostCapabilityRef,
        receiver: Option<&Value>,
        arguments: &[Value],
    ) -> Result<Value, VmError> {
        let cap = match capability.kind {
            HostCapabilityKind::Custom(c) => c,
            HostCapabilityKind::PromiseHook => {
                return crate::modules::async_hooks::promise_hook(&self.state, arguments);
            }
            _ => return Err(VmError::NotCallable),
        };
        dispatch(cap, &self.state, receiver, arguments)
    }

    fn construct(
        &self,
        capability: HostCapabilityRef,
        arguments: &[Value],
    ) -> Result<Value, VmError> {
        let cap = match capability.kind {
            HostCapabilityKind::Custom(c) => c,
            _ => return Err(VmError::NotCallable),
        };
        construct(cap, &self.state, arguments)
    }

    fn construct_with_new_target(
        &self,
        capability: HostCapabilityRef,
        arguments: &[Value],
        _new_target: &Value,
    ) -> Result<Value, VmError> {
        let cap = match capability.kind {
            HostCapabilityKind::Custom(c) => c,
            _ => return Err(VmError::NotCallable),
        };
        construct(cap, &self.state, arguments)
    }
}

fn dispatch(
    cap: CapId,
    state: &Rc<RefCell<HostState>>,
    receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    if let Some(handler) = crate::dispatch::lookup(cap) {
        return handler(state, receiver, args);
    }
    Err(VmError::NotCallable)
}

fn construct(cap: CapId, state: &Rc<RefCell<HostState>>, args: &[Value]) -> Result<Value, VmError> {
    if cap == crate::registry::SPEC_BUFFER_INDEX_OF.cap
        || cap == crate::registry::SPEC_BUFFER_LAST_INDEX_OF.cap
    {
        let method = if cap == crate::registry::SPEC_BUFFER_LAST_INDEX_OF.cap {
            "lastIndexOf"
        } else {
            "indexOf"
        };
        return Err(crate::modules::buffer_enc::invalid_arg_type(format!(
            "The \"buffer\" argument must be an instance of Buffer, TypedArray, or DataView. Received an instance of {method}"
        )));
    }
    if let Some(handler) = crate::dispatch::lookup_construct(cap) {
        return handler(state, args);
    }
    Err(VmError::NotCallable)
}

/// Build a `VmContext` with the Node host pre-installed and every
/// standard Node global/module wired in. The single canonical entry
/// point callers use.
pub fn install(realm: RealmId) -> (Rc<NodeHost>, VmContext) {
    install_with_sink(realm, std::sync::Arc::new(|_| {}))
}

/// Install the host as if invoked as `node <script>`: `process.argv`
/// becomes `[execPath, scriptPath]`.
pub fn install_script(
    realm: RealmId,
    sink: std::sync::Arc<dyn Fn(&str) + Send + Sync>,
    script: &str,
) -> (Rc<NodeHost>, VmContext) {
    let exec_path = host_exec_path();
    install_with_argv(realm, sink, vec![exec_path, script.to_string()])
}

pub fn install_script_with_args(
    realm: RealmId,
    sink: std::sync::Arc<dyn Fn(&str) + Send + Sync>,
    script: &str,
    args: &[String],
) -> (Rc<NodeHost>, VmContext) {
    let exec_path = host_exec_path();
    let argv = std::iter::once(exec_path)
        .chain(std::iter::once(script.to_string()))
        .chain(args.iter().cloned())
        .collect();
    install_with_argv(realm, sink, argv)
}

pub fn install_script_with_args_and_title(
    realm: RealmId,
    sink: std::sync::Arc<dyn Fn(&str) + Send + Sync>,
    script: &str,
    args: &[String],
    title: &str,
) -> (Rc<NodeHost>, VmContext) {
    let exec_path = host_exec_path();
    let argv = std::iter::once(exec_path)
        .chain(std::iter::once(script.to_string()))
        .chain(args.iter().cloned())
        .collect();
    install_with_argv_and_title(realm, sink, argv, title)
}

fn host_exec_path() -> String {
    let executable = std::env::current_exe()
        .ok()
        .and_then(|path| std::fs::canonicalize(path).ok());
    let Some(executable) = executable else {
        return "quench-node".to_string();
    };
    // The compatibility binaries are launchers, but Node exposes the engine
    // identity through `process.execPath`. Keep that fact canonical by using
    // the sibling engine binary when it is present; real `quench-node`
    // invocations continue to report their own executable path.
    let launcher = executable
        .file_stem()
        .and_then(|stem| stem.to_str())
        .is_some_and(|stem| matches!(stem, "run" | "run-compat" | "run-parallel"));
    if launcher {
        let engine = executable.with_file_name("quench-node");
        if engine.is_file() {
            return engine.to_string_lossy().into_owned();
        }
    }
    executable.to_string_lossy().into_owned()
}

/// Whether a shell command names this host or its canonical engine sibling.
/// The compatibility launchers expose the sibling path through
/// `process.execPath`, while the actual child process may still be launched
/// through the current runner executable.
pub(crate) fn command_uses_host_exec(command: &str) -> bool {
    let Some(executable) = std::env::current_exe()
        .ok()
        .and_then(|path| std::fs::canonicalize(path).ok())
    else {
        return false;
    };
    if command.contains(executable.to_string_lossy().as_ref()) {
        return true;
    }
    executable
        .parent()
        .map(|parent| parent.join("quench-node"))
        .filter(|engine| engine.is_file())
        .is_some_and(|engine| command.contains(engine.to_string_lossy().as_ref()))
}

/// Same as `install`, but provides a host-side output sink that
/// receives `console.log/info/...` lines.
pub fn install_with_sink(
    realm: RealmId,
    sink: std::sync::Arc<dyn Fn(&str) + Send + Sync>,
) -> (Rc<NodeHost>, VmContext) {
    install_with_argv(realm, sink, std::env::args().collect())
}

pub fn install_with_argv(
    realm: RealmId,
    sink: std::sync::Arc<dyn Fn(&str) + Send + Sync>,
    argv: Vec<String>,
) -> (Rc<NodeHost>, VmContext) {
    install_with_argv_and_title(realm, sink, argv, "quench-node")
}

/// Install the host with an explicit `process.title` fact. The title is
/// part of the process namespace constructed at install time, so aliases
/// obtained through `require("process")` observe the same value.
pub fn install_with_argv_and_title(
    realm: RealmId,
    sink: std::sync::Arc<dyn Fn(&str) + Send + Sync>,
    argv: Vec<String>,
    title: &str,
) -> (Rc<NodeHost>, VmContext) {
    let exec_argv = std::env::var("QUENCH_EXEC_ARGV")
        .ok()
        .and_then(|value| serde_json::from_str::<Vec<String>>(&value).ok())
        .unwrap_or_default();
    install_with_argv_and_title_and_exec_argv(realm, sink, argv, title, &exec_argv)
}

/// Install a host with explicit Node invocation flags.  `execArgv` is an
/// input fact distinct from `process.argv`; callers such as the upstream test
/// runner use this to carry `// Flags:` metadata before bootstrap executes.
pub fn install_with_argv_and_title_and_exec_argv(
    realm: RealmId,
    sink: std::sync::Arc<dyn Fn(&str) + Send + Sync>,
    argv: Vec<String>,
    title: &str,
    exec_argv: &[String],
) -> (Rc<NodeHost>, VmContext) {
    quench_runtime::date::set_local_timezone(None);
    // Node exposes a default stack-trace limit on the global Error
    // constructor. Keep this host fact available before any internal error
    // constructor captures or formats a stack.
    let error_ctor = quench_runtime::execute::set_property(
        Value::Builtin(quench_runtime::ops::Builtin::Error),
        "stackTraceLimit",
        Value::Number(10.0),
    );
    // Node schedules every async-function continuation as a microtask,
    // including awaits whose operand is already fulfilled.
    quench_runtime::module_bindings::defer_fulfilled_await(true);
    let host = Rc::new(NodeHost::new(realm, argv).with_output_sink(sink));
    host.state.borrow_mut().process.title = title.to_string();
    crate::modules::process::set_abort_on_uncaught_exception(&host.state, exec_argv);
    let host_state = host.state.clone();
    quench_runtime::install_host_job_pump(Rc::new(move || {
        crate::modules::pump::drain_one_tick(&host_state)
    }));
    let (argv, exec_path) = {
        let state = host.state.borrow();
        (state.process.argv.clone(), state.process.exec_path.clone())
    };
    let bindings =
        crate::registry::namespace_bindings_with_exec_argv(&argv, &exec_path, title, exec_argv);
    if let Some((_, process)) = bindings.iter().find(|(name, _)| name == "process") {
        host.state.borrow_mut().process_module = Some(process.clone());
    }
    let mut context = VmContext::default()
        .with_host(host.clone())
        .with_host_value("Error".to_string(), error_ctor);
    // Bootstrap globals derive the public process surface from these
    // canonical argv facts. Keep them identical to the host state so
    // script arguments survive the shared bootstrap path.
    context = context
        .with_host_value(
            "__quench_argv".to_string(),
            host_api::array(argv.iter().cloned().map(Value::String).collect()),
        )
        .with_host_value(
            "__quench_allowed_node_environment_flags".to_string(),
            crate::modules::process::allowed_node_environment_flags(),
        )
        .with_host_value(
            "__quench_error_stack_trace_limit".to_string(),
            Value::Number(10.0),
        )
        .with_host_value(
            "__quench_exec_path".to_string(),
            Value::String(exec_path.clone()),
        )
        // Keep fork's observable process/channel state in the Rust host. The
        // bootstrap module only forwards the public call to this capability.
        .with_host_value(
            "__quench_cp_fork".to_string(),
            crate::host::capability(crate::registry::SPEC_CP_FORK),
        )
        .with_host_value(
            "__quench_cp_spawn_sync".to_string(),
            crate::host::capability(crate::registry::SPEC_CP_SPAWNSYNC),
        )
        .with_host_value(
            "__nodeInternalUtil".to_string(),
            crate::modules::require::internal_util_module(),
        )
        .with_host_value(
            "__quenchHttp2Binding".to_string(),
            crate::modules::http2_util::binding(),
        )
        .with_host_value(
            "__quench_vm_run_in_context".to_string(),
            crate::host::capability(crate::registry::SPEC_VM_RUN_IN_CONTEXT),
        )
        .with_host_value(
            "__quench_vm_run_in_new_context".to_string(),
            crate::host::capability(crate::registry::SPEC_VM_RUN_IN_NEW_CONTEXT),
        )
        .with_host_value(
            "__quench_vm_run_in_this_context".to_string(),
            crate::host::capability(crate::registry::SPEC_VM_RUN_IN_THIS_CONTEXT),
        )
        .with_host_value(
            "__nodeInternalJsStreamSocket".to_string(),
            crate::host::capability(crate::registry::SPEC_INTERNAL_JS_STREAM),
        )
        .with_host_value(
            "__quench_cluster_close_worker".to_string(),
            crate::host::capability(crate::registry::SPEC_CLUSTER_CLOSE_WORKER_NET),
        );
    for (name, value) in bindings {
        context = context.with_host_value(name, value);
    }
    for spec in crate::registry::PERSISTENT_GLOBALS {
        let name = spec.name.rsplit([':', '.']).next().unwrap_or(spec.name);
        context = context.with_persistent_host_value(name, crate::host::capability(*spec));
    }
    // Shared support fragments use these canonical host objects while the
    // fixture is bootstrapping. Keep `__nodePath` identical to require('path')
    // and expose mkdir as the one filesystem capability used by tmpdir.
    let path_module = crate::modules::path::build();
    let fs_module = crate::modules::fs::build();
    host.state()
        .borrow_mut()
        .module_cache
        .insert("path".into(), path_module.clone());
    host.state()
        .borrow_mut()
        .module_cache
        .insert("fs".into(), fs_module.clone());
    context = context
        .with_host_value("__nodePath", path_module)
        .with_host_value("__nodeFs", fs_module)
        .with_host_value(
            "__quenchVfsState",
            host_api::object(vec![("handlers".into(), Value::Null)]),
        )
        .with_host_value(
            "__quench_fs_mkdir",
            crate::host::capability(crate::registry::SPEC_FS_MKDIRSYNC),
        )
        .with_host_value(
            "__quenchInternalTimers",
            host_api::object(vec![
                ("TIMEOUT_MAX".into(), Value::Number(2_147_483_647.0)),
                (
                    "setUnrefTimeout".into(),
                    crate::host::capability(
                        crate::registry::SPEC_INTERNAL_TIMERS_SET_UNREF_TIMEOUT,
                    ),
                ),
                (
                    "async_context_frame".into(),
                    Value::String("Symbol(async_context_frame)\0quench".into()),
                ),
            ]),
        );
    let (url_class, _) = crate::modules::url_whatwg::url_class(&host.state);
    context = context.with_host_value("URL".to_string(), url_class);
    context = context.with_host_value(
        "__quench_dns_lookup".to_string(),
        crate::host::capability(crate::registry::SPEC_DNS_LOOKUP_ADDRESSES),
    );
    let text_decoder = crate::host::capability(crate::registry::SPEC_TEXT_DECODER_NEW);
    context = context.with_host_value("TextDecoder".to_string(), text_decoder);
    let text_encoder = crate::host::capability(crate::registry::SPEC_TEXT_ENCODER_NEW);
    context = context.with_host_value("TextEncoder".to_string(), text_encoder);
    context = context.with_host_value(
        "__quenchGetProxyDetails".to_string(),
        crate::host::capability(crate::registry::SPEC_INTERNAL_GET_PROXY_DETAILS),
    );
    let console = crate::modules::console::build_value();
    host.state.borrow_mut().console_module = Some(console.clone());
    context = context.with_host_value("console".to_string(), console);
    let (crypto, crypto_key) = crate::modules::webcrypto::build();
    let crypto_key_prototype = quench_runtime::execute::get_property(&crypto_key, "prototype");
    context = context
        .with_persistent_host_value("crypto".to_string(), crypto)
        .with_host_value("CryptoKey".to_string(), crypto_key)
        .with_host_value(
            "__quench_crypto_key_prototype".to_string(),
            crypto_key_prototype,
        );
    (host, context)
}

/// Build a capability call descriptor for the host.
pub fn capability(spec: NodeSpec) -> Value {
    let function = host_api::custom_function(RealmId::ROOT, spec.cap);
    let name = spec.name.rsplit([':', '.']).next().unwrap_or(spec.name);
    quench_runtime::execute::set_property(function, "name", Value::String(name.to_string()))
}

/// Build the stable capability descriptor used by bound host callbacks.
pub fn capability_ref(spec: NodeSpec) -> HostCapabilityRef {
    HostCapabilityRef {
        realm: RealmId::ROOT,
        kind: HostCapabilityKind::Custom(spec.cap),
    }
}

/// Build a properly-described namespace object. Each named
/// property is stored as the actual value at `key`, and a
/// parallel descriptor entry is stored under the runtime's
/// `\0quench:descriptor:\0<key>` slot. Calling code reads the
/// value directly from `key`.
pub fn namespace_object(props: Vec<(&str, Value)>) -> Result<Value, VmError> {
    let mut entries: Vec<(String, Value)> = Vec::with_capacity(props.len() * 2);
    for (key, value) in props {
        let descriptor = host_api::object(vec![
            ("value".to_string(), value.clone()),
            ("writable".to_string(), Value::Boolean(true)),
            ("enumerable".to_string(), Value::Boolean(false)),
            ("configurable".to_string(), Value::Boolean(true)),
        ]);
        entries.push((key.to_string(), value));
        entries.push((descriptor_key(key), descriptor));
    }
    Ok(host_api::object(entries))
}

/// Same as `namespace_object`, but takes owned `String` keys.
pub fn namespace_object_from_pairs(props: Vec<(String, Value)>) -> Value {
    let mut entries: Vec<(String, Value)> = Vec::with_capacity(props.len() * 2);
    for (key, value) in props {
        let descriptor = host_api::object(vec![
            ("value".to_string(), value.clone()),
            ("writable".to_string(), Value::Boolean(true)),
            ("enumerable".to_string(), Value::Boolean(true)),
            ("configurable".to_string(), Value::Boolean(true)),
        ]);
        entries.push((key.clone(), value));
        entries.push((descriptor_key(&key), descriptor));
    }
    host_api::object(entries)
}

/// Build a namespace whose data properties cannot be reassigned or deleted.
pub fn readonly_namespace_from_pairs(props: Vec<(String, Value)>) -> Value {
    let mut entries: Vec<(String, Value)> = Vec::with_capacity(props.len() * 2 + 1);
    for (key, value) in props {
        let descriptor = host_api::object(vec![
            ("value".to_string(), value.clone()),
            ("writable".to_string(), Value::Boolean(false)),
            ("enumerable".to_string(), Value::Boolean(true)),
            ("configurable".to_string(), Value::Boolean(false)),
        ]);
        entries.push((key.clone(), value));
        entries.push((descriptor_key(&key), descriptor));
    }
    entries.push(("\0readonly_namespace".into(), Value::Boolean(true)));
    host_api::object(entries)
}

pub fn null_namespace(props: Vec<(String, Value)>) -> Value {
    let object = namespace_object_from_pairs(props);
    let _ = quench_runtime::execute::set_prototype_of(&object, &Value::Null);
    object
}

/// The runtime's descriptor-prefix used by `builtins::descriptor_key`.
fn descriptor_key(key: &str) -> String {
    format!("\0quench:descriptor:\0{key}")
}
