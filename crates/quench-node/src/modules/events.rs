//! `events` module — `EventEmitter` as a pure Rust object.
//!
//! Every emitter is a `NodeObject<EventEmitter>` whose listeners
//! live in a Rust `Vec<Vec<Value>>`. `emit` walks the listeners
//! and calls back into the runtime via the host's call channel.

use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

use quench_runtime::execute::VmError;
use quench_runtime::host_api;
use quench_runtime::value::Value;

use crate::host::HostState;

/// Hidden property that stores the host-side emitter id on
/// the JS Object. Non-enumerable, non-writable.
const EMITTER_ID_PROP: &str = "\0quench:emitter:id";

pub struct EventEmitter {
    pub listeners: HashMap<String, Vec<Value>>,
    pub max: usize,
}

impl EventEmitter {
    pub fn new() -> Self {
        Self {
            listeners: HashMap::new(),
            max: 10,
        }
    }
    pub fn on(&mut self, event: &str, cb: Value) {
        self.listeners.entry(event.to_string()).or_default().push(cb);
    }
    pub fn emit(&self, event: &str) -> Vec<Value> {
        let list = self.listeners.get(event).cloned().unwrap_or_default();
        list.into_iter().take(self.max).collect()
    }
}

/// Stable per-emitter identity. The runtime never sees this
/// directly; the host stores it on the JS Object's descriptor
/// slot and recovers it when `on`/`emit` fire.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct EmitterId(pub u64);

pub struct EmitterRegistry {
    next: u64,
    emitters: HashMap<EmitterId, Rc<RefCell<EventEmitter>>>,
}

impl EmitterRegistry {
    pub fn new() -> Self {
        Self { next: 1, emitters: HashMap::new() }
    }
    pub fn allocate(&mut self) -> EmitterId {
        let id = EmitterId(self.next);
        self.next += 1;
        id
    }
    pub fn get(&self, id: EmitterId) -> Option<Rc<RefCell<EventEmitter>>> {
        self.emitters.get(&id).cloned()
    }
    pub fn insert(&mut self, id: EmitterId, emitter: Rc<RefCell<EventEmitter>>) {
        self.emitters.insert(id, emitter);
    }
}

pub struct EventLoop {
    pub microtasks: RefCell<Vec<(Value, Vec<Value>)>>,
    pub immediates: RefCell<Vec<(Value, Vec<Value>)>>,
}

impl EventLoop {
    pub fn new() -> Self {
        Self {
            microtasks: RefCell::new(Vec::new()),
            immediates: RefCell::new(Vec::new()),
        }
    }

    pub fn queue_microtask(&self, cb: Value, args: Vec<Value>) {
        self.microtasks.borrow_mut().push((cb, args));
    }

    pub fn queue_immediate(&self, cb: Value, args: Vec<Value>) {
        self.immediates.borrow_mut().push((cb, args));
    }

    pub fn drain_microtasks<F>(&self, mut call: F)
    where
        F: FnMut(&Value, &[Value]) -> Result<Value, VmError>,
    {
        loop {
            let snapshot: Vec<_> = self.microtasks.borrow_mut().drain(..).collect();
            if snapshot.is_empty() {
                break;
            }
            for (cb, args) in snapshot {
                let _ = call(&cb, &args);
            }
        }
    }

    pub fn drain_immediates<F>(&self, mut call: F)
    where
        F: FnMut(&Value, &[Value]) -> Result<Value, VmError>,
    {
        loop {
            let snapshot: Vec<_> = self.immediates.borrow_mut().drain(..).collect();
            if snapshot.is_empty() {
                break;
            }
            for (cb, args) in snapshot {
                let _ = call(&cb, &args);
            }
        }
    }
}

pub fn new_emitter(
    state: &Rc<RefCell<HostState>>,
    _args: &[Value],
) -> Result<Value, VmError> {
    let id = state.borrow_mut().emitters.allocate();
    let emitter = Rc::new(RefCell::new(EventEmitter::new()));
    state.borrow_mut().emitters.insert(id, emitter);
    let id_value = Value::Number(id.0 as f64);
    let object = crate::host::namespace_object_from_pairs(vec![
        (EMITTER_ID_PROP.to_string(), id_value),
    ]);
    install_emitter_props(object)
}

fn install_emitter_props(mut object: Value) -> Result<Value, VmError> {
    let props: Vec<(&str, Value)> = vec![
        (
            "on",
            crate::host::capability(crate::registry::NodeSpec::new("events:on", 0x0102)),
        ),
        (
            "addListener",
            crate::host::capability(crate::registry::NodeSpec::new("events:addListener", 0x0104)),
        ),
        (
            "once",
            crate::host::capability(crate::registry::NodeSpec::new("events:once", 0x0105)),
        ),
        (
            "emit",
            crate::host::capability(crate::registry::NodeSpec::new("events:emit", 0x0103)),
        ),
        ("removeListener", cap("events:removeListener", 0x0106)),
        (
            "removeAllListeners",
            cap("events:removeAllListeners", 0x0107),
        ),
        (
            "listeners",
            crate::host::capability(crate::registry::NodeSpec::new("events:listeners", 0x0108)),
        ),
    ];
    for (key, value) in props {
        let descriptor = host_api::object(vec![
            ("value".to_string(), value),
            ("writable".to_string(), Value::Boolean(true)),
            ("enumerable".to_string(), Value::Boolean(false)),
            ("configurable".to_string(), Value::Boolean(true)),
        ]);
        object = quench_runtime::execute::define_property(object, key, descriptor)?;
    }
    Ok(object)
}

fn cap(name: &'static str, id: u16) -> Value {
    crate::host::capability(crate::registry::NodeSpec::new(name, id))
}

pub fn from(_state: &Rc<RefCell<HostState>>, args: &[Value]) -> Result<Value, VmError> {
    let target = args.first().cloned().unwrap_or(Value::Undefined);
    let mut out = Vec::new();
    out.push((
        "on".to_string(),
        crate::host::capability(crate::registry::NodeSpec::new("events:on:method", 0x0102)),
    ));
    out.push((
        "emit".to_string(),
        crate::host::capability(crate::registry::NodeSpec::new("events:emit:method", 0x0103)),
    ));
    out.push(("target".to_string(), target));
    Ok(host_api::object(out))
}

pub fn method_on(
    state: &Rc<RefCell<HostState>>,
    _receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    let Some(receiver) = _receiver else {
        return Ok(Value::Undefined);
    };
    let Some(id) = emitter_id(receiver) else {
        return Ok(Value::Undefined);
    };
    let event = match args.first() {
        Some(Value::String(s)) => s.clone(),
        _ => return Ok(Value::Undefined),
    };
    let cb = match args.get(1).cloned() {
        Some(v) => v,
        None => return Ok(Value::Undefined),
    };
    if let Some(emitter) = state.borrow().emitters.get(id) {
        emitter.borrow_mut().on(&event, cb);
    }
    Ok(Value::Undefined)
}

pub fn method_emit(
    state: &Rc<RefCell<HostState>>,
    _receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    let Some(receiver) = _receiver else {
        return Ok(Value::Boolean(false));
    };
    let Some(id) = emitter_id(receiver) else {
        return Ok(Value::Boolean(false));
    };
    let event = match args.first() {
        Some(Value::String(s)) => s.clone(),
        _ => return Ok(Value::Boolean(false)),
    };
    let rest: Vec<Value> = args.get(1..).unwrap_or(&[]).to_vec();
    let listeners = take_listeners(state, id, &event);
    let count = listeners.len();
    for cb in listeners {
        let _ = quench_runtime::execute::call(&cb, receiver, &rest);
    }
    Ok(Value::Boolean(count > 0))
}

fn take_listeners(
    state: &Rc<RefCell<HostState>>,
    id: EmitterId,
    event: &str,
) -> Vec<Value> {
    let event = event.to_string();
    state
        .borrow()
        .emitters
        .get(id)
        .map(|e| e.borrow().emit(&event))
        .unwrap_or_default()
}

fn emitter_id(receiver: &Value) -> Option<EmitterId> {
    let v = quench_runtime::vm::get_property(receiver, EMITTER_ID_PROP);
    match v {
        Value::Number(n) if n.is_finite() && n >= 0.0 => Some(EmitterId(n as u64)),
        _ => None,
    }
}

/// Shared thunk used by `setTimeout` / `setImmediate` to push a
/// callback into the host event loop.
pub fn enqueue_callback(state: &Rc<RefCell<HostState>>, cb: Value, args: Vec<Value>) {
    state.borrow_mut().event_loop.queue_immediate(cb, args);
}

/// One callable that the host can invoke from inside the runtime.
pub fn make_callback(_state: &Rc<RefCell<HostState>>, _cb: Value) -> Value {
    crate::host::capability(crate::registry::NodeSpec::new("events:noop", 0x01FF))
}

pub fn build() -> Value {
    crate::host::namespace_object(vec![(
        "EventEmitter",
        crate::host::capability(crate::registry::SPEC_EVENTS_NEW),
    )])
    .unwrap_or_else(|_| Value::Undefined)
}
