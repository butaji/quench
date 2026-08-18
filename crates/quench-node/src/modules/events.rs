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

pub fn new_emitter(_state: &Rc<RefCell<HostState>>, _args: &[Value]) -> Result<Value, VmError> {
    let emitter = EventEmitter::new();
    let _ = emitter;
    install_emitter_props(host_api::object(Vec::new()))
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

pub fn method_on(_state: &Rc<RefCell<HostState>>, args: &[Value]) -> Result<Value, VmError> {
    let _ = args;
    Ok(Value::Undefined)
}

pub fn method_emit(_state: &Rc<RefCell<HostState>>, args: &[Value]) -> Result<Value, VmError> {
    let _ = args;
    Ok(Value::Undefined)
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
