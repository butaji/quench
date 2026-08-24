//! `EventTarget` and the `events` module statics.
//!
//! `EventTarget` mirrors the DOM/Node contract: listeners are keyed
//! by type, the same `(type, callback)` pair registers once, and
//! `{ once: true }` listeners are removed after they fire.
//! `getMaxListeners`/`setMaxListeners`/`getEventListeners` accept
//! both `EventEmitter` and `EventTarget` instances, like Node.

use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

use quench_runtime::execute::{self, VmError};
use quench_runtime::host_api;
use quench_runtime::value::Value;

use crate::host::HostState;
use crate::modules::emitter::{emitter_id, EmitterId, Listener};

/// Hidden property storing the host-side target id on the JS object.
const TARGET_ID_PROP: &str = "\0quench:event-target:id";
/// Hidden brand marking `AbortSignal` objects; their max-listener
/// count is always 0 in Node.
pub const ABORT_SIGNAL_BRAND: &str = "\0quench:abort:signal";

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct TargetId(pub u64);

#[derive(Default)]
pub struct EventTarget {
    /// Insertion-ordered `(type, listeners)` pairs.
    pub events: Vec<(String, Vec<Listener>)>,
    pub max: Option<usize>,
}

impl EventTarget {
    fn entry(&mut self, event: &str) -> &mut Vec<Listener> {
        match self.events.iter().position(|(key, _)| key == event) {
            Some(index) => &mut self.events[index].1,
            None => {
                self.events.push((event.to_string(), Vec::new()));
                &mut self.events.last_mut().expect("just pushed").1
            }
        }
    }

    fn listeners_of(&self, event: &str) -> &[Listener] {
        self.events
            .iter()
            .find(|(key, _)| key == event)
            .map(|(_, list)| list.as_slice())
            .unwrap_or(&[])
    }
}

#[derive(Default)]
pub struct TargetRegistry {
    next: u64,
    targets: HashMap<TargetId, Rc<RefCell<EventTarget>>>,
}

impl TargetRegistry {
    pub fn new() -> Self {
        Self {
            next: 1,
            targets: HashMap::new(),
        }
    }
    fn allocate(&mut self) -> TargetId {
        let id = TargetId(self.next);
        self.next += 1;
        id
    }
    pub fn get(&self, id: TargetId) -> Option<Rc<RefCell<EventTarget>>> {
        self.targets.get(&id).cloned()
    }
}

fn target_id(receiver: &Value) -> Option<TargetId> {
    match quench_runtime::vm::get_property(receiver, TARGET_ID_PROP) {
        Value::Number(n) if n.is_finite() && n >= 0.0 => Some(TargetId(n as u64)),
        _ => None,
    }
}

fn is_abort_signal(value: &Value) -> bool {
    matches!(
        quench_runtime::vm::get_property(value, ABORT_SIGNAL_BRAND),
        Value::Boolean(true)
    )
}

pub fn new_target(state: &Rc<RefCell<HostState>>, _args: &[Value]) -> Result<Value, VmError> {
    let id = state.borrow_mut().targets.allocate();
    let target = Rc::new(RefCell::new(EventTarget::default()));
    state.borrow_mut().targets.targets.insert(id, target);
    let object = crate::host::namespace_object_from_pairs(vec![(
        TARGET_ID_PROP.to_string(),
        Value::Number(id.0 as f64),
    )]);
    install_target_props(object)
}

fn install_target_props(mut object: Value) -> Result<Value, VmError> {
    for (key, value) in target_props() {
        let descriptor = host_api::object(vec![
            ("value".to_string(), value),
            ("writable".to_string(), Value::Boolean(true)),
            ("enumerable".to_string(), Value::Boolean(false)),
            ("configurable".to_string(), Value::Boolean(true)),
        ]);
        object = execute::define_property(object, key, descriptor)?;
    }
    Ok(object)
}

fn target_props() -> Vec<(&'static str, Value)> {
    vec![
        (
            "addEventListener",
            cap("event_target:addEventListener", 0x0113),
        ),
        (
            "removeEventListener",
            cap("event_target:removeEventListener", 0x0114),
        ),
        ("dispatchEvent", cap("event_target:dispatchEvent", 0x0115)),
    ]
}

fn cap(name: &'static str, id: u16) -> Value {
    crate::host::capability(crate::registry::NodeSpec::new(name, id))
}

fn type_arg(args: &[Value]) -> Result<String, VmError> {
    match args.first() {
        Some(Value::String(name)) => Ok(name.clone()),
        _ => Err(execute::type_error(
            "The \"type\" argument must be of type string",
        )),
    }
}

fn callback_arg(args: &[Value]) -> Result<Value, VmError> {
    match args.get(1) {
        Some(Value::Null | Value::Undefined) => Ok(args[1].clone()),
        Some(value) if quench_runtime::is_callable(value) || matches!(value, Value::Object(_)) => {
            Ok(value.clone())
        }
        _ => Err(execute::type_error(
            "The \"callback\" argument must be of type function",
        )),
    }
}

fn same_listener(left: &Value, right: &Value) -> bool {
    let left_object = matches!(left, Value::Object(_));
    let right_object = matches!(right, Value::Object(_));
    left_object == right_object
        && quench_runtime::is_callable(left) == quench_runtime::is_callable(right)
        && execute::same_value(left, right)
}

/// `{ once: true }` from the options bag; anything else is ignored.
fn once_option(args: &[Value]) -> bool {
    matches!(
        args.get(2).and_then(|v| execute::get_property_result(v, "once").ok()),
        Some(Value::Boolean(true))
    )
}

fn passive_option(args: &[Value]) -> bool {
    args.get(2)
        .and_then(|v| execute::get_property_result(v, "passive").ok())
        .is_some_and(|value| execute::is_truthy(&value))
}

fn signal_option(args: &[Value]) -> Result<Option<Value>, VmError> {
    let Some(options) = args.get(2) else { return Ok(None) };
    if matches!(options, Value::Null | Value::Undefined) {
        return Ok(None);
    }
    let signal = execute::get_property_result(options, "signal")?;
    if matches!(signal, Value::Undefined) {
        return Ok(None);
    }
    if !matches!(signal, Value::Object(_)) || !is_abort_signal(&signal) {
        return Err(execute::type_error("The \"signal\" option must be an AbortSignal"));
    }
    Ok(Some(signal))
}

fn weak_option(args: &[Value], receiver: &Value) -> bool {
    let Some(options) = args.get(2) else {
        return false;
    };
    let handler = execute::get_property(options, "kWeakHandler\0quench");
    matches!(handler, Value::Object(_)) && !execute::same_value(&handler, receiver)
}

pub fn add_event_listener(
    state: &Rc<RefCell<HostState>>,
    receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    let event = type_arg(args)?;
    let callback = callback_arg(args)?;
    let signal = signal_option(args)?;
    if signal
        .as_ref()
        .is_some_and(|value| execute::is_truthy(&execute::get_property(value, "aborted")))
    {
        return Ok(Value::Undefined);
    }
    let Some(target) = receiver
        .and_then(target_id)
        .and_then(|id| state.borrow().targets.get(id))
    else {
        return Ok(Value::Undefined);
    };
    let mut guard = target.borrow_mut();
    let existing = guard.listeners_of(&event);
    if !existing
        .iter()
        .any(|listener| same_listener(&listener.callback, &callback))
    {
        guard.entry(&event).push(Listener {
            callback,
            once: once_option(args),
            weak: weak_option(args, receiver.expect("validated receiver")),
            passive: passive_option(args),
            signal,
        });
    }
    Ok(Value::Undefined)
}

pub fn remove_event_listener(
    state: &Rc<RefCell<HostState>>,
    receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    let event = type_arg(args)?;
    let callback = callback_arg(args)?;
    if let Some(target) = receiver
        .and_then(target_id)
        .and_then(|id| state.borrow().targets.get(id))
    {
        let mut guard = target.borrow_mut();
        if let Some(index) = guard.events.iter().position(|(key, _)| key == &event) {
            let list = &mut guard.events[index].1;
            if let Some(at) = list
                .iter()
                .position(|listener| execute::same_value(&listener.callback, &callback))
            {
                list.remove(at);
            }
        }
    }
    Ok(Value::Undefined)
}

pub fn dispatch_event(
    state: &Rc<RefCell<HostState>>,
    receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    let (Some(receiver), Some(event)) = (receiver, args.first()) else {
        return Err(execute::type_error(
            "The \"event\" argument must be an instance of Event",
        ));
    };
    if !matches!(event, Value::Object(_))
        || matches!(execute::get_property(event, "type"), Value::Undefined)
    {
        return Err(execute::type_error(
            "The \"event\" argument must be an instance of Event",
        ));
    }
    let Some(target) = target_id(receiver).and_then(|id| state.borrow().targets.get(id)) else {
        return Ok(Value::Boolean(false));
    };
    let Value::String(event_type) = execute::get_property(event, "type") else {
        return Ok(Value::Boolean(false));
    };
    if event_type == "abort" && is_abort_signal(receiver) {
        let mut host = state.borrow_mut();
        for target in host.targets.targets.values() {
            for (_, listeners) in &mut target.borrow_mut().events {
                listeners.retain(|listener| {
                    listener
                        .signal
                        .as_ref()
                        .is_none_or(|signal| target_id(signal) != target_id(receiver))
                });
            }
        }
    }
    let snapshot: Vec<Listener> = target.borrow().listeners_of(&event_type).to_vec();
    let has_protected_listener = snapshot.iter().any(|listener| {
        execute::is_truthy(&execute::get_property(
            &listener.callback,
            "\0quench:abort-listener",
        ))
    });
    let active = execute::set_property(
        execute::set_property(
            execute::set_property(event.clone(), "target", receiver.clone()),
            "currentTarget",
            receiver.clone(),
        ),
        "srcElement",
        receiver.clone(),
    );
    let active = execute::set_property(active, "eventPhase", Value::Number(2.0));
    execute::replace_value(event, &active);
    for listener in &snapshot {
        if listener.weak {
            continue;
        }
        if listener
            .signal
            .as_ref()
            .is_some_and(|signal| execute::is_truthy(&execute::get_property(signal, "aborted")))
        {
            continue;
        }
        let protected = execute::is_truthy(&execute::get_property(
            &listener.callback,
            "\0quench:abort-listener",
        ));
        let stopped = execute::is_truthy(&execute::get_property(event, "\0event:cancelBubble"))
            || event
                .object_identity()
                .is_some_and(|identity| state.borrow().stopped_events.contains(&identity));
        if stopped && has_protected_listener && !protected {
            continue;
        }
        if listener.once {
            remove_event_listener(
                state,
                Some(receiver),
                &[Value::String(event_type.clone()), listener.callback.clone()],
            )?;
        }
        if listener.passive {
            let passive = execute::set_property(
                event.clone(),
                "\0event:passive",
                Value::Boolean(true),
            );
            execute::replace_value(event, &passive);
        }
        let result = if quench_runtime::is_callable(&listener.callback) {
            execute::call(&listener.callback, receiver, std::slice::from_ref(event))
        } else if let Value::Object(_) = &listener.callback {
            let handler = execute::get_property(&listener.callback, "handleEvent");
            if quench_runtime::is_callable(&handler) {
                execute::call(&handler, &listener.callback, std::slice::from_ref(event))
            } else {
                Ok(Value::Undefined)
            }
        } else {
            Ok(Value::Undefined)
        };
        if let Err(error) = result {
            crate::modules::pump::handle_uncaught(state, error)?;
            crate::modules::pump::run_uncaught(state)?;
        }
        if !has_protected_listener
            && (execute::is_truthy(&execute::get_property(event, "\0event:cancelBubble"))
                || event
                    .object_identity()
                    .is_some_and(|identity| state.borrow().stopped_events.contains(&identity)))
        {
            break;
        }
    }
    let prevented = event
        .object_identity()
        .is_some_and(|identity| state.borrow().prevented_events.contains(&identity))
        || execute::is_truthy(&execute::get_property(event, "defaultPrevented"));
    let reset = execute::set_property(event.clone(), "eventPhase", Value::Number(0.0));
    let reset = execute::set_property(reset, "currentTarget", Value::Null);
    let reset = execute::set_property(reset, "srcElement", Value::Null);
    let reset = execute::set_property(reset, "target", receiver.clone());
    execute::replace_value(event, &reset);
    Ok(Value::Boolean(!prevented))
}

/// Node-style invalid-argument TypeError carrying a `code`.
fn invalid_emitter_error(got: &Value) -> VmError {
    VmError::Thrown(host_api::object(vec![
        ("name".to_string(), Value::String("TypeError".to_string())),
        (
            "message".to_string(),
            Value::String(format!(
                "The \"emitter\" argument must be an instance of EventEmitter or EventTarget. Received {}",
                crate::modules::util::inspect(got)
            )),
        ),
        (
            "code".to_string(),
            Value::String("ERR_INVALID_ARG_TYPE".to_string()),
        ),
    ]))
}

/// `events.getEventListeners(emitterOrTarget, name)`.
pub fn get_event_listeners(
    state: &Rc<RefCell<HostState>>,
    _receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    let target = args.first().cloned().unwrap_or(Value::Undefined);
    let event = match args.get(1) {
        Some(Value::String(name)) => name.clone(),
        _ => String::new(),
    };
    if let Some(callbacks) = emitter_callbacks(state, &target, &event) {
        return Ok(host_api::array(callbacks));
    }
    if let Some(callbacks) = target_callbacks(state, &target, &event) {
        return Ok(host_api::array(callbacks));
    }
    Err(invalid_emitter_error(&target))
}

fn emitter_callbacks(
    state: &Rc<RefCell<HostState>>,
    value: &Value,
    event: &str,
) -> Option<Vec<Value>> {
    let id: EmitterId = emitter_id(value)?;
    let emitter = state.borrow().emitters.get(id)?;
    let guard = emitter.borrow();
    Some(
        guard
            .listeners_of(event)
            .iter()
            .map(|listener| listener.callback.clone())
            .collect(),
    )
}

fn target_callbacks(
    state: &Rc<RefCell<HostState>>,
    value: &Value,
    event: &str,
) -> Option<Vec<Value>> {
    let id = target_id(value)?;
    let target = state.borrow().targets.get(id)?;
    let guard = target.borrow();
    Some(
        guard
            .listeners_of(event)
            .iter()
            .map(|listener| listener.callback.clone())
            .collect(),
    )
}

/// `events.getMaxListeners(emitterOrTarget)`.
pub fn get_max_listeners(
    state: &Rc<RefCell<HostState>>,
    _receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    let target = args.first().cloned().unwrap_or(Value::Undefined);
    if is_abort_signal(&target) {
        return Ok(Value::Number(0.0));
    }
    let default = state.borrow().emitters.default_max;
    if let Some(id) = emitter_id(&target) {
        if let Some(emitter) = state.borrow().emitters.get(id) {
            let max = emitter.borrow().max.unwrap_or(default);
            return Ok(Value::Number(max as f64));
        }
    }
    if let Some(id) = target_id(&target) {
        if let Some(t) = state.borrow().targets.get(id) {
            let max = t.borrow().max.unwrap_or(default);
            return Ok(Value::Number(max as f64));
        }
    }
    Err(invalid_emitter_error(&target))
}

/// `events.setMaxListeners(n, ...targets)`; with no targets it sets
/// the process-wide default.
pub fn set_max_listeners(
    state: &Rc<RefCell<HostState>>,
    _receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    let n = match args.first() {
        Some(Value::Number(n)) if *n >= 0.0 && n.is_finite() => *n as usize,
        _ => {
            return Err(execute::type_error(
                "The \"n\" argument must be a non-negative number",
            ))
        }
    };
    let targets = args.get(1..).unwrap_or(&[]);
    if targets.is_empty() {
        state.borrow_mut().emitters.default_max = n;
        return Ok(Value::Undefined);
    }
    for target in targets {
        apply_max_listeners(state, target, n)?;
    }
    Ok(Value::Undefined)
}

fn apply_max_listeners(
    state: &Rc<RefCell<HostState>>,
    target: &Value,
    n: usize,
) -> Result<(), VmError> {
    if let Some(id) = emitter_id(target) {
        if let Some(emitter) = state.borrow().emitters.get(id) {
            emitter.borrow_mut().max = Some(n);
            return Ok(());
        }
    }
    if let Some(id) = target_id(target) {
        if let Some(t) = state.borrow().targets.get(id) {
            t.borrow_mut().max = Some(n);
            return Ok(());
        }
    }
    Err(invalid_emitter_error(target))
}

/// `events.listenerCount(emitter, event)` static.
pub fn listener_count(
    state: &Rc<RefCell<HostState>>,
    _receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    let target = args.first().cloned().unwrap_or(Value::Undefined);
    let event = match args.get(1) {
        Some(Value::String(name)) => name.clone(),
        _ => String::new(),
    };
    if let Some(callbacks) = emitter_callbacks(state, &target, &event) {
        return Ok(Value::Number(callbacks.len() as f64));
    }
    if let Some(callbacks) = target_callbacks(state, &target, &event) {
        return Ok(Value::Number(callbacks.len() as f64));
    }
    Err(invalid_emitter_error(&target))
}
