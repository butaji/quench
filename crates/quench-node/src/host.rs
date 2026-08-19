//! Host trait implementation. One `NodeHost` impl, one dispatch.
//!
//! Builtins return `Value::Object` they own (plain Rust objects
//! exposed through the runtime's ordinary object semantics). The
//! host never re-enters the VM for state — every state lives in
//! the Rust envelope.

use std::cell::RefCell;
use std::rc::Rc;

use quench_runtime::execute::VmError;
use quench_runtime::host_api;
use quench_runtime::ops::{HostCapabilityKind, HostCapabilityRef, RealmId};
use quench_runtime::value::Value;
use quench_runtime::vm::{Host, OutputSink, VmContext};

use crate::registry::{CapId, NodeSpec};

pub struct NodeHost {
    state: Rc<RefCell<HostState>>,
}

pub struct HostState {
    pub timers: crate::modules::timers::TimerRegistry,
    pub event_loop: crate::modules::events::EventLoop,
    pub process: crate::modules::process::ProcessState,
    pub fs: crate::modules::fs::FsState,
    pub net: crate::modules::net::NetState,
    pub http: crate::modules::http::HttpState,
    pub emitters: crate::modules::events::EmitterRegistry,
    pub output: Option<OutputSink>,
    pub realm: RealmId,
    /// Directory stack for the CJS loader: top is the requiring module's dir.
    pub dir_stack: Vec<String>,
    /// CJS module cache keyed by canonical file path.
    pub module_cache: std::collections::HashMap<String, Value>,
    /// Module record handed to `__quench_cjs_wrap__` for the file
    /// currently being loaded by `require`.
    pub pending_module: Option<PendingModule>,
}

/// Host-side handoff record for one in-flight CJS module load.
pub struct PendingModule {
    pub module: Value,
    pub filename: String,
    pub dirname: String,
}

impl NodeHost {
    pub fn new(realm: RealmId) -> Self {
        let state = HostState {
            timers: crate::modules::timers::TimerRegistry::new(),
            event_loop: crate::modules::events::EventLoop::new(),
            process: crate::modules::process::ProcessState::new(),
            fs: crate::modules::fs::FsState::new(),
            net: crate::modules::net::NetState::new(),
            http: crate::modules::http::HttpState::new(),
            emitters: crate::modules::events::EmitterRegistry::new(),
            output: None,
            realm,
            dir_stack: Vec::new(),
            module_cache: std::collections::HashMap::new(),
            pending_module: None,
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

/// Same as `install`, but provides a host-side output sink that
/// receives `console.log/info/...` lines.
pub fn install_with_sink(
    realm: RealmId,
    sink: std::sync::Arc<dyn Fn(&str) + Send + Sync>,
) -> (Rc<NodeHost>, VmContext) {
    let host = Rc::new(NodeHost::new(realm).with_output_sink(sink));
    let mut context = VmContext::default().with_host(host.clone());
    let bindings = crate::registry::namespace_bindings();
    for (name, value) in bindings {
        context = context.with_host_value(name, value);
    }
    (host, context)
}

/// Build a capability call descriptor for the host.
pub fn capability(spec: NodeSpec) -> Value {
    host_api::custom_function(RealmId::ROOT, spec.cap)
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

/// The runtime's descriptor-prefix used by `builtins::descriptor_key`.
fn descriptor_key(key: &str) -> String {
    format!("\0quench:descriptor:\0{key}")
}
