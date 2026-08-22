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

pub fn scheduler_capability(kind: u16) -> Value {
    quench_runtime::host_api::capability_function(HostCapabilityRef {
        realm: RealmId::ROOT,
        kind: HostCapabilityKind::Custom(kind),
    })
}
pub struct NodeHost {
    state: Rc<RefCell<HostState>>,
}

pub struct HostState {
    pub timers: crate::modules::timers::TimerRegistry,
    pub event_loop: crate::modules::event_loop::EventLoop,
    pub process: crate::modules::process::ProcessState,
    pub fs: crate::modules::fs::FsState,
    pub net: crate::modules::net::NetState,
    pub http: crate::modules::http::HttpState,
    pub emitters: crate::modules::emitter::EmitterRegistry,
    pub targets: crate::modules::event_target::TargetRegistry,
    pub output: Option<OutputSink>,
    pub realm: RealmId,
    /// Directory stack for the CJS loader: top is the requiring module's dir.
    pub dir_stack: Vec<String>,
    /// CJS module cache keyed by canonical file path.
    pub module_cache: std::collections::HashMap<String, Value>,
    /// Stack of module records handed to `__quench_cjs_wrap__`.  A stack is
    /// required because module evaluation can re-enter the loader.
    pub pending_modules: Vec<PendingModule>,
    /// Thrown value stashed by `pump::handle_uncaught`, dispatched by
    /// the `__quench_uncaught__` capability inside an active frame.
    pub pending_uncaught: Option<Value>,
    /// Shared `URL` class pair (constructor, prototype), built on first use
    /// so `instanceof URL` has one canonical prototype per realm.
    pub url_class: Option<(Value, Value)>,
    /// `require('stream')` module value, evaluated once from the
    /// embedded JS prelude (`modules/stream_prelude.js`).
    pub stream_module: Option<Value>,
    /// `require('async_hooks')` module value, evaluated once from the
    /// embedded JS factory (`modules/async_hooks.js`).
    pub async_hooks_module: Option<Value>,
    pub node_test_module: Option<Value>,
    /// `require('http-errors')` module value, evaluated once from the
    /// embedded JS factory (`modules/http_errors.js`).
    pub http_errors_module: Option<Value>,
    /// `require('statuses')` module value, evaluated once from the
    /// embedded JS factory (`modules/statuses.js`).
    pub statuses_module: Option<Value>,
    /// `require('punycode')` module value, evaluated once from the
    /// embedded JS factory (`modules/punycode.js`).
    pub punycode_module: Option<Value>,
    pub perf_hooks_module: Option<Value>,
    pub trace_events_module: Option<Value>,
    /// `require('express')` module value (real Express-compatible
    /// `createApplication` factory), evaluated once from the embedded JS
    /// factory (`modules/express.js`).
    pub express_module: Option<Value>,
    /// `require('express/lib/request')` / `node:express/request`
    /// module value (real Express request prototype factory),
    /// evaluated once from the embedded JS factory
    /// (`modules/express_request.js`).
    pub express_request_module: Option<Value>,
    pub mime_db_module: Option<Value>,
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
            timers: crate::modules::timers::TimerRegistry::new(),
            event_loop: crate::modules::event_loop::EventLoop::new(),
            process: crate::modules::process::ProcessState::new(argv),
            fs: crate::modules::fs::FsState::new(),
            net: crate::modules::net::NetState::new(),
            http: crate::modules::http::HttpState::new(),
            emitters: crate::modules::emitter::EmitterRegistry::new(),
            targets: crate::modules::event_target::TargetRegistry::new(),
            output: None,
            realm,
            dir_stack: Vec::new(),
            module_cache: std::collections::HashMap::new(),
            pending_modules: Vec::new(),
            pending_uncaught: None,
            url_class: None,
            stream_module: None,
            async_hooks_module: None,
            node_test_module: None,
            punycode_module: None,
            http_errors_module: None,
            statuses_module: None,
            express_module: None,
            perf_hooks_module: None,
            trace_events_module: None,
            express_request_module: None,
            mime_db_module: None,
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
    if matches!(cap, 2348 | 2349)
        && !receiver.is_some_and(|value| {
            quench_runtime::execute::get_property_result(value, "__quench_scheduler_identity")
                .ok()
                .is_some_and(|marker| marker == Value::Boolean(true))
        })
    {
        return Err(VmError::Thrown(scheduler_error(
            "ERR_INVALID_THIS",
            "Value of \"this\" must be of type Scheduler",
        )));
    }
    match cap {
        2348 => dispatch(2346, state, None, &[
            args.first().cloned().unwrap_or(Value::Number(0.0)),
            Value::Undefined,
            args.get(1).cloned().unwrap_or(Value::Undefined),
        ]),
        2349 => dispatch(2347, state, None, &[
            Value::Undefined,
            args.first().cloned().unwrap_or(Value::Undefined),
        ]),
        2350 => Err(VmError::Thrown(scheduler_error(
            "ERR_ILLEGAL_CONSTRUCTOR",
            "Illegal constructor",
        ))),
        _ => {
            if let Some(handler) = crate::dispatch::lookup(cap) {
                return handler(state, receiver, args);
            }
            Err(VmError::NotCallable)
        }
    }
}

fn scheduler_error(code: &str, message: &str) -> Value {
    quench_runtime::host_api::object(vec![
        ("name".into(), Value::String("TypeError".into())),
        ("message".into(), Value::String(message.into())),
        ("code".into(), Value::String(code.into())),
    ])
}

fn construct(cap: CapId, state: &Rc<RefCell<HostState>>, args: &[Value]) -> Result<Value, VmError> {
    if cap == 2350 {
        return Err(VmError::Thrown(scheduler_error(
            "ERR_ILLEGAL_CONSTRUCTOR",
            "Illegal constructor",
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
    let exec_path = std::env::current_exe()
        .map(|p| p.to_string_lossy().into_owned())
        .unwrap_or_default();
    install_with_argv(realm, sink, vec![exec_path, script.to_string()])
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
    let host = Rc::new(NodeHost::new(realm, argv).with_output_sink(sink));
    let (argv, exec_path) = {
        let state = host.state.borrow();
        (state.process.argv.clone(), state.process.exec_path.clone())
    };
    let bindings = crate::registry::namespace_bindings(&argv, &exec_path);
    let mut context = VmContext::default().with_host(host.clone());
    for (name, value) in bindings {
        context = context.with_host_value(name, value);
    }
    context = context.with_host_value("crypto".to_string(), crate::modules::crypto::build());
    let (url_class, _) = crate::modules::url_whatwg::url_class(&host.state);
    context = context.with_host_value("URL".to_string(), url_class);
    let url_search_params = crate::host::capability(crate::registry::SPEC_URL_SEARCHPARAMS_NEW);
    context = context.with_host_value("URLSearchParams".to_string(), url_search_params);
    context = install_web_globals(context);
    let text_decoder = crate::host::capability(crate::registry::SPEC_TEXT_DECODER_NEW);
    let context = context.with_host_value("TextDecoder".to_string(), text_decoder);
    let text_encoder = crate::host::capability(crate::registry::SPEC_TEXT_ENCODER_NEW);
    (
        host,
        context.with_host_value("TextEncoder".to_string(), text_encoder),
    )
}

fn install_web_globals(mut context: VmContext) -> VmContext {
    if let Ok(web) = crate::modules::web_globals::build() {
        for name in [
            "Headers",
            "FormData",
            "Blob",
            "Event",
            "CustomEvent",
            "DOMException",
            "MessageChannel",
            "MessagePort",
            "BroadcastChannel",
            "ReadableStream",
            "WritableStream",
            "TransformStream",
            "TextDecoderStream",
            "TextEncoderStream",
            "CompressionStream",
            "DecompressionStream",
            "Request",
            "Response",
        ] {
            let value = quench_runtime::execute::get_property(&web, name);
            context = context.with_host_value(name.to_string(), value);
        }
    }
    context
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

/// Flip one property's descriptor to enumerable on a namespace object
/// (the rest keep their non-enumerable default). Node exports like
/// `fs.promises` are enumerable; record the descriptor slot in place.
/// Flip one property's descriptor to enumerable on a namespace object,
/// returning the mutated object. Consumes `object` so the `Rc` is
/// uniquely owned and mutable in place.
pub fn make_property_enumerable(mut object: Value, key: &str) -> Value {
    let Value::Object(data) = &mut object else {
        return object;
    };
    let Some(inner) = Rc::get_mut(data) else {
        return object;
    };
    let dkey = descriptor_key(key);
    if let Some((_, Value::Object(desc))) = inner.iter_mut().find(|(k, _)| k == &dkey) {
        let Some(desc_inner) = Rc::get_mut(desc) else {
            return object;
        };
        for (k, v) in desc_inner.iter_mut() {
            if k == "enumerable" {
                *v = Value::Boolean(true);
                return object;
            }
        }
    }
    object
}
