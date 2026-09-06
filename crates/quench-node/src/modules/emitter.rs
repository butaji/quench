//! Emitter state: the `EventEmitter` listener store and the
//! host-side registry that maps JS objects to it.
//!
//! Event names are strings, including the engine's symbol-encoded
//! strings. `once` listeners carry a flag; `emit` removes them
//! before their callback runs, mirroring Node's wrapper.

use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

use quench_runtime::execute;
use quench_runtime::value::Value;

/// Hidden property that stores the host-side emitter id on
/// the JS Object. Non-enumerable, non-writable.
pub const EMITTER_ID_PROP: &str = "\0quench:emitter:id";

/// One registered listener. `once` listeners are removed by `emit`
/// before their callback is invoked.
#[derive(Clone)]
pub struct Listener {
    pub callback: Value,
    pub once: bool,
    /// Logical process that registered this listener. Shared handles (for
    /// example a transferred net.Server) dispatch only to the active scope.
    pub process_scope: u64,
    /// EventTarget registration identity includes the capture flag. EventEmitter
    /// listeners always use the default `false` value.
    pub capture: bool,
    /// NodeEventTarget's `addEventListener` receives an Event object from
    /// `emit`; EventEmitter-style methods receive the raw argument list.
    pub node_event: bool,
    pub weak: bool,
    pub passive: bool,
    pub signal: Option<Value>,
}

pub struct EventEmitter {
    /// Insertion-ordered `(event name, listeners)` pairs; order is
    /// observable through `eventNames()`.
    pub events: Vec<(String, Vec<Listener>)>,
    /// Per-emitter override of the max-listener count.
    pub max: Option<usize>,
    /// Route rejected promises returned by listeners to the error channel.
    pub capture_rejections: bool,
}

impl Default for EventEmitter {
    fn default() -> Self {
        Self::new()
    }
}

impl EventEmitter {
    pub fn new() -> Self {
        Self {
            events: Vec::new(),
            max: None,
            capture_rejections: false,
        }
    }

    pub fn entry(&mut self, event: &str) -> &mut Vec<Listener> {
        match self.events.iter().position(|(key, _)| key == event) {
            Some(index) => &mut self.events[index].1,
            None => {
                self.events.push((event.to_string(), Vec::new()));
                &mut self.events.last_mut().expect("just pushed").1
            }
        }
    }

    pub fn add(
        &mut self,
        event: &str,
        callback: Value,
        once: bool,
        prepend: bool,
        process_scope: u64,
    ) -> usize {
        let list = self.entry(event);
        if prepend {
            list.insert(
                0,
                Listener {
                    callback,
                    once,
                    process_scope,
                    capture: false,
                    node_event: false,
                    weak: false,
                    passive: false,
                    signal: None,
                },
            );
        } else {
            list.push(Listener {
                callback,
                once,
                process_scope,
                capture: false,
                node_event: false,
                weak: false,
                passive: false,
                signal: None,
            });
        }
        list.len()
    }

    pub fn listeners_of(&self, event: &str) -> &[Listener] {
        self.events
            .iter()
            .find(|(key, _)| key == event)
            .map(|(_, list)| list.as_slice())
            .unwrap_or(&[])
    }

    pub fn listeners_for_scope(&self, event: &str, process_scope: u64) -> Vec<Listener> {
        self.listeners_of(event)
            .iter()
            .filter(|listener| listener.process_scope == process_scope)
            .cloned()
            .collect()
    }

    /// Remove the most recently added listener equal to `callback`.
    pub fn remove(&mut self, event: &str, callback: &Value) -> bool {
        if let Some(index) = self.events.iter().position(|(key, _)| key == event) {
            let list = &mut self.events[index].1;
            if let Some(at) = list
                .iter()
                .rposition(|listener| execute::same_value(&listener.callback, callback))
            {
                list.remove(at);
                if list.is_empty() {
                    self.events.remove(index);
                }
                return true;
            }
        }
        false
    }

    pub fn remove_for_scope(&mut self, event: &str, callback: &Value, process_scope: u64) -> bool {
        if let Some(index) = self.events.iter().position(|(key, _)| key == event) {
            let list = &mut self.events[index].1;
            if let Some(at) = list.iter().rposition(|listener| {
                listener.process_scope == process_scope
                    && execute::same_value(&listener.callback, callback)
            }) {
                list.remove(at);
                if list.is_empty() {
                    self.events.remove(index);
                }
                return true;
            }
        }
        false
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
    /// Semantic object identity, not an `Rc` address. Runtime copy-on-write
    /// replacements preserve this id, so host callbacks keep the same state.
    identities: HashMap<u64, EmitterId>,
    /// `events.defaultMaxListeners`.
    pub default_max: usize,
}

impl Default for EmitterRegistry {
    fn default() -> Self {
        Self::new()
    }
}

impl EmitterRegistry {
    pub fn new() -> Self {
        Self {
            next: 1,
            emitters: HashMap::new(),
            identities: HashMap::new(),
            default_max: 10,
        }
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

    pub fn identity(&self, value: &Value) -> Option<EmitterId> {
        value
            .object_identity()
            .and_then(|key| self.identities.get(&key).copied())
    }

    pub fn bind_identity(&mut self, value: &Value, id: EmitterId) {
        if let Some(key) = value.object_identity() {
            self.identities.insert(key, id);
        }
    }
}

/// Resolve the emitter id stored on a JS receiver object.
pub fn emitter_id(receiver: &Value) -> Option<EmitterId> {
    let own = quench_runtime::execute::execute_builtin_with_receiver(
        quench_runtime::ops::Builtin::ObjectHasOwnProperty,
        &[Value::String(EMITTER_ID_PROP.to_string())],
        Some(receiver),
    )
    .ok()
    .is_some_and(|value| matches!(value, Value::Boolean(true)));
    if !own {
        return None;
    }
    match quench_runtime::vm::get_property(receiver, EMITTER_ID_PROP) {
        Value::Number(n) if n.is_finite() && n >= 0.0 => Some(EmitterId(n as u64)),
        _ => None,
    }
}
