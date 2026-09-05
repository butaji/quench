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
    /// Thrown value stashed by `pump::handle_uncaught`, dispatched by
    /// the `__quench_uncaught__` capability inside an active frame.
    pub pending_uncaught: Option<Value>,
    /// Shared `URL` class pair (constructor, prototype), built on first use
    /// so `instanceof URL` has one canonical prototype per realm.
    pub url_class: Option<(Value, Value)>,
    /// `require('stream')` module value, evaluated once from the
    /// embedded JS prelude (`modules/stream_prelude.js`).
    pub stream_module: Option<Value>,
    pub string_decoder_aliases: std::collections::HashMap<u64, u64>,
    pub string_decoder_pending: std::collections::HashMap<u64, Vec<u8>>,
    pub string_decoder_encoding: std::collections::HashMap<u64, String>,
    pub string_decoder_next_id: u64,
    /// Canonical `internalBinding("os")` object for this realm.
    pub os_binding: Option<Value>,
    /// Canonical `internalBinding("cares_wrap")` object for this realm.
    pub cares_binding: Option<Value>,
    /// Composite AbortSignals keyed by each source target identity.
    pub abort_composites: std::collections::HashMap<u64, Vec<Value>>,
    /// Strong roots for host-created identity-bearing objects exposed as aliases.
    pub identity_roots: Vec<Value>,
    /// Canonical child-process prototype kept out of the JavaScript global.
    pub child_process_prototype: Option<Value>,
}

/// Host-side handoff record for one in-flight CJS module load.
pub struct PendingModule {
    pub module: Value,
    pub filename: String,
    pub dirname: String,
}

impl NodeHost {
    pub fn new(realm: RealmId, argv: Vec<String>) -> Self {
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
            pending_uncaught: None,
            url_class: None,
            stream_module: None,
            string_decoder_aliases: std::collections::HashMap::new(),
            string_decoder_pending: std::collections::HashMap::new(),
            string_decoder_encoding: std::collections::HashMap::new(),
            string_decoder_next_id: 1,
            os_binding: None,
            cares_binding: None,
            abort_composites: std::collections::HashMap::new(),
            identity_roots: Vec::new(),
            child_process_prototype: None,
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

fn host_exec_path() -> String {
    std::env::current_exe()
        .ok()
        .and_then(|path| std::fs::canonicalize(path).ok())
        .map(|path| path.to_string_lossy().into_owned())
        .unwrap_or_else(|| "quench-node".to_string())
}

/// Same as `install`, but provides a host-side output sink that
/// receives raw output chunks from `console` and process streams.
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
    // Node schedules every async-function continuation as a microtask,
    // including awaits whose operand is already fulfilled.
    quench_runtime::module_bindings::defer_fulfilled_await(true);
    let host = Rc::new(NodeHost::new(realm, argv).with_output_sink(sink.clone()));
    let host_state = host.state.clone();
    quench_runtime::install_host_job_pump(Rc::new(move || {
        crate::modules::pump::drain_one_tick(&host_state)
    }));
    let (argv, exec_path) = {
        let state = host.state.borrow();
        (state.process.argv.clone(), state.process.exec_path.clone())
    };
    let bindings = crate::registry::namespace_bindings(&argv, &exec_path);
    let mut context = VmContext::with_output_sink(sink).with_host(host.clone());
    // Bootstrap globals derive the public process surface from these
    // canonical argv facts. Keep them identical to the host state so
    // script arguments survive the shared bootstrap path.
    context = context
        .with_host_value(
            "__quench_argv".to_string(),
            host_api::array(argv.iter().cloned().map(Value::String).collect()),
        )
        .with_host_value(
            "__quench_exec_path".to_string(),
            Value::String(exec_path.clone()),
        );
    for (name, value) in bindings {
        context = context.with_host_value(name, value);
    }
    for spec in crate::registry::PERSISTENT_GLOBALS {
        let name = spec.name.rsplit([':', '.']).next().unwrap_or(spec.name);
        context = context.with_persistent_host_value(name, crate::host::capability(*spec));
    }
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
    let blob_prototype = host_api::object(vec![]);
    let blob = host_api::bound_builtin(quench_runtime::ops::Builtin::Object, Value::Undefined);
    let blob = quench_runtime::execute::set_property(blob, "prototype", blob_prototype.clone());
    let file_prototype = host_api::object(vec![]);
    let _ = quench_runtime::execute::set_prototype_of(&file_prototype, &blob_prototype);
    let file = host_api::bound_builtin(quench_runtime::ops::Builtin::Object, Value::Undefined);
    let file = quench_runtime::execute::set_property(file, "prototype", file_prototype);
    context = context
        .with_host_value("Blob".to_string(), blob)
        .with_host_value("File".to_string(), file);
    let console = crate::modules::console::build_value();
    context = context.with_host_value("console".to_string(), console);
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
    let mut entries: Vec<(String, Value)> = Vec::with_capacity(props.len() * 2);
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
