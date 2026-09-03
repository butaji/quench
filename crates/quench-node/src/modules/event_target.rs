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
    /// Whether this target exposes NodeEventTarget's EventEmitter-style
    /// methods in addition to the DOM dispatch contract.
    pub node: bool,
    /// Insertion-ordered `(type, listeners)` pairs.
    pub events: Vec<(String, Vec<Listener>)>,
    pub max: Option<usize>,
    /// Whether the Node-style listener warning has already fired.
    pub warned: bool,
    /// Resource kind used for Node's observable warning label.
    pub message_port: bool,
}

thread_local! {
    static NODE_PROTOTYPE: RefCell<Option<Value>> = const { RefCell::new(None) };
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

pub(crate) fn target_identity(receiver: &Value) -> Option<u64> {
    target_id(receiver).map(|id| id.0)
}

fn is_abort_signal(value: &Value) -> bool {
    matches!(
        quench_runtime::vm::get_property(value, ABORT_SIGNAL_BRAND),
        Value::Boolean(true)
    )
}

pub fn new_target(state: &Rc<RefCell<HostState>>, _args: &[Value]) -> Result<Value, VmError> {
    allocate_target(state, false)
}

/// Keep the constructor and allocated targets on one prototype fact. Bootstrap
/// modules can expose the constructor before its host prototype is installed;
/// repair that derived view once, at the boundary, instead of teaching every
/// consumer a separate fallback.
pub(crate) fn ensure_constructor_prototype(constructor: &Value) -> Value {
    let current = execute::get_property(constructor, "prototype");
    if quench_runtime::is_callable(&execute::get_property(&current, "addEventListener")) {
        return current;
    }
    let replacement = prototype();
    let _ = execute::set_callable_property(constructor, "prototype", replacement.clone());
    replacement
}

/// Construct the internal NodeEventTarget using the same target registry and
/// listener records as EventTarget. Only the method surface differs.
pub fn new_node_target(state: &Rc<RefCell<HostState>>, _args: &[Value]) -> Result<Value, VmError> {
    allocate_target(state, true)
}

fn allocate_target(state: &Rc<RefCell<HostState>>, node: bool) -> Result<Value, VmError> {
    let id = state.borrow_mut().targets.allocate();
    let target = Rc::new(RefCell::new(EventTarget {
        node,
        ..EventTarget::default()
    }));
    state.borrow_mut().targets.targets.insert(id, target);
    let object = crate::host::namespace_object_from_pairs(vec![(
        TARGET_ID_PROP.to_string(),
        Value::Number(id.0 as f64),
    )]);
    let prototype = if node {
        NODE_PROTOTYPE
            .with(|slot| slot.borrow().clone())
            .unwrap_or_else(prototype)
    } else {
        let global = quench_runtime::vm::current_global_object();
        let constructor = execute::get_property(&global, "EventTarget");
        ensure_constructor_prototype(&constructor)
    };
    let object = if matches!(prototype, Value::Object(_) | Value::ObjectAlias(_)) {
        execute::set_prototype_of(&object, &prototype)?
    } else {
        object
    };
    Ok(object)
}

pub fn new_message_channel(state: &Rc<RefCell<HostState>>) -> Result<Value, VmError> {
    let port1 = allocate_target(state, false)?;
    let port2 = allocate_target(state, false)?;
    for port in [&port1, &port2] {
        if let Some(id) = target_id(port) {
            if let Some(target) = state.borrow().targets.get(id) {
                target.borrow_mut().message_port = true;
            }
        }
    }
    Ok(host_api::object(vec![
        ("port1".into(), port1),
        ("port2".into(), port2),
    ]))
}

pub(crate) fn set_node_prototype(prototype: Value) {
    NODE_PROTOTYPE.with(|slot| *slot.borrow_mut() = Some(prototype));
}

pub(crate) fn prototype() -> Value {
    let mut prototype = host_api::object(Vec::new());
    for (name, value) in target_props() {
        prototype = execute::define_property(
            prototype,
            name,
            host_api::object(vec![
                ("value".into(), value),
                ("writable".into(), Value::Boolean(true)),
                ("enumerable".into(), Value::Boolean(true)),
                ("configurable".into(), Value::Boolean(true)),
            ]),
        )
        .expect("EventTarget prototype property definition");
    }
    execute::define_property(
        prototype,
        "Symbol.toStringTag",
        host_api::object(vec![
            ("value".into(), Value::String("EventTarget".into())),
            ("writable".into(), Value::Boolean(false)),
            ("enumerable".into(), Value::Boolean(false)),
            ("configurable".into(), Value::Boolean(true)),
        ]),
    )
    .expect("EventTarget toStringTag definition")
}

fn target_props() -> Vec<(&'static str, Value)> {
    vec![
        (
            "addEventListener",
            crate::host::capability(crate::registry::SPEC_TARGET_ADD),
        ),
        (
            "dispatchEvent",
            crate::host::capability(crate::registry::SPEC_TARGET_DISPATCH),
        ),
        (
            "removeEventListener",
            crate::host::capability(crate::registry::SPEC_TARGET_REMOVE),
        ),
    ]
}

fn node_target(
    state: &Rc<RefCell<HostState>>,
    receiver: Option<&Value>,
) -> Result<Rc<RefCell<EventTarget>>, VmError> {
    let Some(receiver) = receiver else {
        return Err(invalid_this());
    };
    let Some(target) = target_id(receiver).and_then(|id| state.borrow().targets.get(id)) else {
        return Err(invalid_this());
    };
    if !target.borrow().node {
        return Err(invalid_this());
    }
    Ok(target)
}

fn node_register(
    state: &Rc<RefCell<HostState>>,
    receiver: Option<&Value>,
    args: &[Value],
    once: bool,
    node_event: bool,
) -> Result<Value, VmError> {
    let target = node_target(state, receiver)?;
    let event = type_arg(args)?;
    let callback = callback_arg(args)?;
    if matches!(callback, Value::Null | Value::Undefined) {
        return Ok(receiver.cloned().unwrap_or(Value::Undefined));
    }
    let (count, limit, already_warned, inserted) = {
        let mut target = target.borrow_mut();
        if target
            .listeners_of(&event)
            .iter()
            .any(|listener| same_listener(&listener.callback, &callback))
        {
            (
                target.listeners_of(&event).len(),
                target.max,
                target.warned,
                false,
            )
        } else {
            target.entry(&event).push(Listener {
                callback,
                once,
                capture: false,
                node_event,
                weak: false,
                passive: false,
                signal: None,
            });
            (
                target.listeners_of(&event).len(),
                target.max,
                target.warned,
                true,
            )
        }
    };
    let limit = limit.unwrap_or(10);
    if inserted && count > limit && limit > 0 && !already_warned {
        if let Some(target) = receiver
            .and_then(target_id)
            .and_then(|id| state.borrow().targets.get(id))
        {
            target.borrow_mut().warned = true;
        }
        warn_max_listeners(
            state,
            receiver.expect("validated receiver"),
            &event,
            count,
            limit,
        );
    }
    Ok(receiver.cloned().unwrap_or(Value::Undefined))
}

/// Queue the one-per-target NodeEventTarget listener warning for process
/// warning handlers. The warning is an Error instance with the observable
/// `target`, `count`, and `type` fields Node exposes.
fn warn_max_listeners(
    state: &Rc<RefCell<HostState>>,
    target: &Value,
    event: &str,
    count: usize,
    limit: usize,
) {
    let target_kind = target_id(target).and_then(|id| {
        state.borrow().targets.get(id).map(|target| {
            let target = target.borrow();
            (target.node, target.message_port)
        })
    });
    let label = if is_abort_signal(target) {
        "[AbortSignal]"
    } else if target_kind.is_some_and(|(_, message_port)| message_port) {
        "[MessagePort [EventTarget]]"
    } else if target_kind.is_some_and(|(node, _)| node) {
        "NodeEventTarget"
    } else {
        "EventTarget"
    };
    let message = format!(
        "Possible EventTarget memory leak detected. {count} {event} listeners added to {label}. MaxListeners is {}. Use events.setMaxListeners() to increase limit",
        limit
    );
    let warning = quench_runtime::builtins::error(
        quench_runtime::ops::Builtin::Error,
        &[Value::String(message)],
    );
    let warning = execute::set_property(
        warning,
        "name",
        Value::String("MaxListenersExceededWarning".into()),
    );
    let warning = execute::set_property(warning, "target", target.clone());
    let warning = execute::set_property(warning, "count", Value::Number(count as f64));
    let warning = execute::set_property(warning, "type", Value::String(event.into()));
    // Deliver through the canonical process warning transition. This keeps
    // EventTarget warnings on the same state machine as process.emitWarning,
    // including listener identity and once-handler removal semantics.
    let emitter = crate::host::capability(crate::registry::SPEC_PROCESS_EMIT);
    state
        .borrow_mut()
        .event_loop
        .queue_microtask(emitter, vec![Value::String("warning".into()), warning]);
}

pub fn node_add_event_listener(
    state: &Rc<RefCell<HostState>>,
    receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    let _ = node_register(state, receiver, args, once_option(args), true)?;
    Ok(Value::Undefined)
}

pub fn node_on(
    state: &Rc<RefCell<HostState>>,
    receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    node_register(state, receiver, args, false, false)
}

pub fn node_once(
    state: &Rc<RefCell<HostState>>,
    receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    node_register(state, receiver, args, true, false)
}

pub fn node_remove_listener(
    state: &Rc<RefCell<HostState>>,
    receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    let _ = node_target(state, receiver)?;
    remove_event_listener(state, receiver, args)?;
    Ok(receiver.cloned().unwrap_or(Value::Undefined))
}

pub fn node_remove_all_listeners(
    state: &Rc<RefCell<HostState>>,
    receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    let target = node_target(state, receiver)?;
    let event = match args.first() {
        Some(Value::String(value)) => Some(value.clone()),
        Some(Value::Undefined) | None => None,
        _ => return Err(type_arg(args).unwrap_err()),
    };
    let mut target = target.borrow_mut();
    if let Some(event) = event {
        target.events.retain(|(name, _)| name != &event);
    } else {
        target.events.clear();
    }
    Ok(receiver.cloned().unwrap_or(Value::Undefined))
}

pub fn node_listener_count(
    state: &Rc<RefCell<HostState>>,
    receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    let target = node_target(state, receiver)?;
    let event = type_arg(args)?;
    let count = target.borrow().listeners_of(&event).len();
    Ok(Value::Number(count as f64))
}

pub fn node_event_names(
    state: &Rc<RefCell<HostState>>,
    receiver: Option<&Value>,
    _args: &[Value],
) -> Result<Value, VmError> {
    let target = node_target(state, receiver)?;
    let names = target
        .borrow()
        .events
        .iter()
        .filter(|(_, listeners)| !listeners.is_empty())
        .map(|(name, _)| Value::String(name.clone()))
        .collect();
    Ok(host_api::array(names))
}

pub fn node_set_max_listeners(
    state: &Rc<RefCell<HostState>>,
    receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    let target = node_target(state, receiver)?;
    let value = args.first().ok_or_else(missing_args)?;
    let Value::Number(number) = value else {
        return Err(invalid_arg_type(
            "n",
            "number",
            value,
            "The \"n\" argument must be a number.",
        ));
    };
    if !number.is_finite() || *number < 0.0 {
        return Err(invalid_arg_type(
            "n",
            "non-negative number",
            value,
            "The \"n\" argument must be a non-negative number.",
        ));
    }
    target.borrow_mut().max = Some(*number as usize);
    Ok(receiver.cloned().unwrap_or(Value::Undefined))
}

pub fn node_get_max_listeners(
    state: &Rc<RefCell<HostState>>,
    receiver: Option<&Value>,
    _args: &[Value],
) -> Result<Value, VmError> {
    let target = node_target(state, receiver)?;
    let max = target.borrow().max.unwrap_or(10);
    Ok(Value::Number(max as f64))
}

pub fn node_emit(
    state: &Rc<RefCell<HostState>>,
    receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    let receiver = receiver.ok_or_else(invalid_this)?;
    let target = node_target(state, Some(receiver))?;
    let event = match args.first() {
        Some(_) => type_arg(args)?,
        None => {
            return Err(invalid_arg_type(
                "type",
                "string",
                &Value::Undefined,
                "The \"type\" argument must be a string.",
            ));
        }
    };
    let detail = args.get(1).cloned().unwrap_or(Value::Undefined);
    let event_value = crate::dispatch_handlers::custom_event_new(
        state,
        &[
            Value::String(event.clone()),
            host_api::object(vec![("detail".into(), detail.clone())]),
        ],
    )?;
    let snapshot = target.borrow().listeners_of(&event).to_vec();
    let has_listeners = !snapshot.is_empty();
    for listener in snapshot {
        if listener.once {
            let _ = remove_event_listener(
                state,
                Some(receiver),
                &[Value::String(event.clone()), listener.callback.clone()],
            );
        }
        let value = if listener.node_event {
            event_value.clone()
        } else {
            detail.clone()
        };
        let result = if quench_runtime::is_callable(&listener.callback) {
            execute::call(&listener.callback, receiver, &[value])
        } else {
            let handler = execute::get_property(&listener.callback, "handleEvent");
            if quench_runtime::is_callable(&handler) {
                execute::call(&handler, &listener.callback, &[value])
            } else {
                Ok(Value::Undefined)
            }
        };
        match result {
            Ok(Value::Promise(promise)) => {
                let rejection =
                    crate::host::capability(crate::registry::SPEC_EVENT_TARGET_REJECTION);
                quench_runtime::promise_then(
                    Some(&Value::Promise(promise)),
                    &[Value::Undefined, rejection],
                )?;
            }
            Ok(_) => {}
            Err(error) => {
                crate::modules::pump::handle_uncaught(state, error)?;
                crate::modules::pump::run_uncaught(state)?;
            }
        }
    }
    Ok(Value::Boolean(has_listeners))
}

pub(crate) fn node_prototype() -> Value {
    let mut prototype = host_api::object(Vec::new());
    for (name, value) in [
        (
            "addEventListener",
            crate::host::capability(crate::registry::SPEC_NODE_EVENT_TARGET_ADD),
        ),
        (
            "removeEventListener",
            crate::host::capability(crate::registry::SPEC_NODE_EVENT_TARGET_REMOVE),
        ),
        (
            "dispatchEvent",
            crate::host::capability(crate::registry::SPEC_NODE_EVENT_TARGET_DISPATCH),
        ),
        (
            "on",
            crate::host::capability(crate::registry::SPEC_NODE_EVENT_TARGET_ON),
        ),
        (
            "addListener",
            crate::host::capability(crate::registry::SPEC_NODE_EVENT_TARGET_ON),
        ),
        (
            "once",
            crate::host::capability(crate::registry::SPEC_NODE_EVENT_TARGET_ONCE),
        ),
        (
            "off",
            crate::host::capability(crate::registry::SPEC_NODE_EVENT_TARGET_REMOVE),
        ),
        (
            "removeListener",
            crate::host::capability(crate::registry::SPEC_NODE_EVENT_TARGET_REMOVE),
        ),
        (
            "removeAllListeners",
            crate::host::capability(crate::registry::SPEC_NODE_EVENT_TARGET_REMOVE_ALL),
        ),
        (
            "listenerCount",
            crate::host::capability(crate::registry::SPEC_NODE_EVENT_TARGET_LISTENER_COUNT),
        ),
        (
            "eventNames",
            crate::host::capability(crate::registry::SPEC_NODE_EVENT_TARGET_EVENT_NAMES),
        ),
        (
            "setMaxListeners",
            crate::host::capability(crate::registry::SPEC_NODE_EVENT_TARGET_SET_MAX),
        ),
        (
            "getMaxListeners",
            crate::host::capability(crate::registry::SPEC_NODE_EVENT_TARGET_GET_MAX),
        ),
        (
            "emit",
            crate::host::capability(crate::registry::SPEC_NODE_EVENT_TARGET_EMIT),
        ),
    ] {
        execute::set_property_in_place(&prototype, name, value);
    }
    prototype
}

fn type_arg(args: &[Value]) -> Result<String, VmError> {
    match args.first() {
        Some(Value::String(name)) if !name.contains('\0') && !name.contains("ymbol") => {
            Ok(name.clone())
        }
        Some(value) => Err(invalid_arg_type(
            "type",
            "string",
            value,
            "The \"type\" argument must be of type string.",
        )),
        None => Err(missing_args()),
    }
}

fn callback_arg(args: &[Value]) -> Result<Value, VmError> {
    match args.get(1) {
        Some(Value::Null | Value::Undefined) => Ok(args[1].clone()),
        Some(value) if quench_runtime::is_callable(value) || matches!(value, Value::Object(_)) => {
            Ok(value.clone())
        }
        Some(value) => Err(invalid_arg_type(
            "callback",
            "function",
            value,
            "The \"listener\" argument must be an instance of EventListener.",
        )),
        None => Err(missing_args()),
    }
}

fn invalid_arg_type(_name: &str, _expected: &str, value: &Value, prefix: &str) -> VmError {
    let received = match value {
        Value::Null => " Received null".to_string(),
        Value::Undefined => " Received undefined".to_string(),
        Value::Boolean(value) => format!(" Received type boolean ({value})"),
        Value::Number(value) => format!(" Received type number ({value})"),
        Value::String(value) => format!(
            " Received type string ({})",
            crate::modules::util::inspect(&Value::String(value.clone()))
        ),
        value if quench_runtime::is_callable(value) => " Received function".to_string(),
        Value::Object(_) => " Received an instance of Object".to_string(),
        value => format!(" Received {}", crate::modules::util::inspect(value)),
    };
    coded_error(
        quench_runtime::ops::Builtin::TypeError,
        "ERR_INVALID_ARG_TYPE",
        &format!("{prefix}{received}"),
    )
}

fn invalid_this() -> VmError {
    coded_error(
        quench_runtime::ops::Builtin::TypeError,
        "ERR_INVALID_THIS",
        "Value of \"this\" must be an instance of EventTarget",
    )
}

fn missing_args() -> VmError {
    coded_error(
        quench_runtime::ops::Builtin::TypeError,
        "ERR_MISSING_ARGS",
        "The \"type\" argument is required",
    )
}

fn coded_error(builtin: quench_runtime::ops::Builtin, code: &str, message: &str) -> VmError {
    let error = quench_runtime::builtins::error(builtin, &[Value::String(message.to_string())]);
    VmError::Thrown(execute::set_property(
        error,
        "code",
        Value::String(code.to_string()),
    ))
}

fn queue_listener_warning(state: &Rc<RefCell<HostState>>, receiver: &Value) {
    let warning = host_api::object(vec![
        (
            "name".into(),
            Value::String("AddEventListenerArgumentTypeWarning".into()),
        ),
        (
            "message".into(),
            Value::String("The listener argument must be a function or an object".into()),
        ),
        ("target".into(), receiver.clone()),
    ]);
    state.borrow_mut().event_loop.queue_microtask(
        crate::host::capability(crate::registry::SPEC_PROCESS_EMIT),
        vec![Value::String("warning".into()), warning],
    );
}

fn event_recursion() -> VmError {
    coded_error(
        quench_runtime::ops::Builtin::Error,
        "ERR_EVENT_RECURSION",
        "The event is already being dispatched",
    )
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
        args.get(2)
            .and_then(|v| execute::get_property_result(v, "once").ok()),
        Some(Value::Boolean(true))
    )
}

fn passive_option(args: &[Value]) -> bool {
    args.get(2)
        .and_then(|v| execute::get_property_result(v, "passive").ok())
        .is_some_and(|value| execute::is_truthy(&value))
}

fn capture_option(args: &[Value]) -> bool {
    args.get(2)
        .and_then(|v| execute::get_property_result(v, "capture").ok())
        .is_some_and(|value| execute::is_truthy(&value))
}

fn signal_option(args: &[Value]) -> Result<Option<Value>, VmError> {
    let Some(options) = args.get(2) else {
        return Ok(None);
    };
    if matches!(options, Value::Null | Value::Undefined) {
        return Ok(None);
    }
    let signal = execute::get_property_result(options, "signal")?;
    if matches!(signal, Value::Undefined) {
        return Ok(None);
    }
    if !matches!(signal, Value::Object(_)) || !is_abort_signal(&signal) {
        return Err(execute::type_error(
            "The \"signal\" option must be an AbortSignal",
        ));
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

fn protected_abort_listener(callback: &Value) -> bool {
    if matches!(
        callback,
        Value::BoundFunction(bound)
            if matches!(
                bound.target,
                Value::Builtin(quench_runtime::ops::Builtin::HostCapability(
                    quench_runtime::ops::HostCapabilityKind::Custom(0x0130)
                ))
            )
    ) {
        return true;
    }
    execute::is_truthy(&execute::get_property(callback, "\0quench:abort-listener"))
}

pub fn add_event_listener(
    state: &Rc<RefCell<HostState>>,
    receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    if receiver
        .and_then(|value| target_id(value))
        .and_then(|id| state.borrow().targets.get(id))
        .is_none()
    {
        return Err(invalid_this());
    }
    let event = type_arg(args)?;
    // Node probes the options bag before validating the listener, so a
    // passive getter is observable even when the listener is null.
    let passive = passive_option(args);
    let callback = match callback_arg(args) {
        Ok(callback) => callback,
        Err(error) => {
            if let Some(receiver) = receiver {
                queue_listener_warning(state, receiver);
            }
            return Err(error);
        }
    };
    if matches!(callback, Value::Null | Value::Undefined) {
        if let Some(receiver) = receiver {
            queue_listener_warning(state, receiver);
        }
        return Ok(Value::Undefined);
    }
    let signal = signal_option(args)?;
    let capture = capture_option(args);
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
        return Err(invalid_this());
    };
    let weak = weak_option(args, receiver.expect("validated receiver"));
    let (count, limit, should_warn, inserted) = {
        let mut guard = target.borrow_mut();
        let existing = guard.listeners_of(&event);
        if existing.iter().any(|listener| {
            same_listener(&listener.callback, &callback) && listener.capture == capture
        }) {
            (
                existing.iter().filter(|listener| !listener.weak).count(),
                guard
                    .max
                    .unwrap_or_else(|| state.borrow().emitters.default_max),
                false,
                false,
            )
        } else {
            guard.entry(&event).push(Listener {
                callback,
                once: once_option(args),
                capture,
                node_event: false,
                weak,
                passive,
                signal,
            });
            let count = guard
                .listeners_of(&event)
                .iter()
                .filter(|listener| !listener.weak)
                .count();
            let limit = guard.max.unwrap_or_else(|| {
                if is_abort_signal(receiver.expect("validated receiver")) {
                    0
                } else {
                    state.borrow().emitters.default_max
                }
            });
            let should_warn = count > limit && limit > 0 && !guard.warned;
            if should_warn {
                guard.warned = true;
            }
            (count, limit, should_warn, true)
        }
    };
    if inserted && !weak {
        execute::set_property_in_place(
            receiver.expect("validated receiver"),
            "\0quench:weak-listener",
            Value::Boolean(true),
        );
    }
    if inserted && should_warn {
        warn_max_listeners(
            state,
            receiver.expect("validated receiver"),
            &event,
            count,
            limit,
        );
    }
    Ok(Value::Undefined)
}

pub fn remove_event_listener(
    state: &Rc<RefCell<HostState>>,
    receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    if receiver
        .and_then(|value| target_id(value))
        .and_then(|id| state.borrow().targets.get(id))
        .is_none()
    {
        return Err(invalid_this());
    }
    let event = type_arg(args)?;
    let callback = callback_arg(args)?;
    let Some(target) = receiver
        .and_then(target_id)
        .and_then(|id| state.borrow().targets.get(id))
    else {
        return Err(invalid_this());
    };
    {
        let mut guard = target.borrow_mut();
        if let Some(index) = guard.events.iter().position(|(key, _)| key == &event) {
            let list = &mut guard.events[index].1;
            let capture = capture_option(args);
            if let Some(at) = list.iter().position(|listener| {
                execute::same_value(&listener.callback, &callback) && listener.capture == capture
            }) {
                list.remove(at);
            }
            if list.is_empty() {
                if let Some(receiver) = receiver {
                    execute::set_property_in_place(
                        receiver,
                        "\0quench:weak-listener",
                        Value::Boolean(false),
                    );
                }
            }
        }
    }
    Ok(Value::Undefined)
}

pub fn replace_event_listener(
    state: &Rc<RefCell<HostState>>,
    receiver: &Value,
    event: &str,
    old: &Value,
    replacement: &Value,
) -> bool {
    let Some(target) = target_id(receiver).and_then(|id| state.borrow().targets.get(id)) else {
        return false;
    };
    let mut guard = target.borrow_mut();
    let Some(list) = guard
        .events
        .iter_mut()
        .find(|(key, _)| key == event)
        .map(|(_, listeners)| listeners)
    else {
        return false;
    };
    let Some(index) = list
        .iter()
        .position(|listener| execute::same_value(&listener.callback, old))
    else {
        return false;
    };
    list[index].callback = replacement.clone();
    true
}

fn listener_is_registered(
    state: &Rc<RefCell<HostState>>,
    receiver: &Value,
    event: &str,
    callback: &Value,
) -> bool {
    target_id(receiver)
        .and_then(|id| state.borrow().targets.get(id))
        .is_some_and(|target| {
            target
                .borrow()
                .listeners_of(event)
                .iter()
                .any(|listener| execute::same_value(&listener.callback, callback))
        })
}

pub fn dispatch_event(
    state: &Rc<RefCell<HostState>>,
    receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    let Some(receiver) = receiver else {
        return Err(invalid_this());
    };
    let Some(target) = target_id(receiver).and_then(|id| state.borrow().targets.get(id)) else {
        return Err(invalid_this());
    };
    let Some(event) = args.first() else {
        return Err(missing_args());
    };
    if !matches!(event, Value::Object(_))
        || matches!(execute::get_property(event, "type"), Value::Undefined)
    {
        return Err(invalid_arg_type(
            "event",
            "Event",
            event,
            "The \"event\" argument must be an instance of Event.",
        ));
    }
    let Value::String(event_type) = execute::get_property(event, "type") else {
        return Ok(Value::Boolean(false));
    };
    let event_identity = event.object_identity();
    if let Some(identity) = event_identity {
        let mut guard = state.borrow_mut();
        if !guard.dispatching_events.insert(identity) {
            return Err(event_recursion());
        }
    }
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
    let has_protected_listener = snapshot
        .iter()
        .any(|listener| protected_abort_listener(&listener.callback));
    execute::set_property_in_place(event, "target", receiver.clone());
    execute::set_property_in_place(event, "currentTarget", receiver.clone());
    execute::set_property_in_place(event, "srcElement", receiver.clone());
    execute::set_property_in_place(event, "eventPhase", Value::Number(2.0));
    for listener in &snapshot {
        if listener.weak {
            continue;
        }
        if !listener_is_registered(state, receiver, &event_type, &listener.callback) {
            continue;
        }
        if listener
            .signal
            .as_ref()
            .is_some_and(|signal| execute::is_truthy(&execute::get_property(signal, "aborted")))
        {
            continue;
        }
        let protected = protected_abort_listener(&listener.callback);
        let stopped = execute::is_truthy(&execute::get_property(event, "\0event:cancelBubble"))
            || event
                .object_identity()
                .is_some_and(|identity| state.borrow().stopped_events.contains(&identity));
        if stopped && (!has_protected_listener || !protected) {
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
            execute::set_property_in_place(event, "\0event:passive", Value::Boolean(true));
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
        match result {
            Ok(Value::Promise(promise)) => {
                // EventTarget reports rejected async listeners through the
                // uncaught-exception channel, rather than as unhandled
                // rejections.  Installing the rejection reaction here also
                // records the promise as handled before the generic pump.
                let rejection =
                    crate::host::capability(crate::registry::SPEC_EVENT_TARGET_REJECTION);
                quench_runtime::promise_then(
                    Some(&Value::Promise(promise)),
                    &[Value::Undefined, rejection],
                )?;
            }
            Ok(_) => {}
            Err(error) => {
                crate::modules::pump::handle_uncaught(state, error)?;
                crate::modules::pump::run_uncaught(state)?;
            }
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
    let prevented = execute::is_truthy(&execute::get_property(event, "defaultPrevented"));
    execute::set_property_in_place(event, "eventPhase", Value::Number(0.0));
    execute::set_property_in_place(event, "currentTarget", Value::Null);
    execute::set_property_in_place(event, "srcElement", Value::Null);
    execute::set_property_in_place(event, "target", receiver.clone());
    if let Some(identity) = event_identity {
        state.borrow_mut().dispatching_events.remove(&identity);
    }
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
        Some(Value::Number(n)) => {
            return Err(crate::modules::buffer_enc::out_of_range(
                "n",
                "a non-negative number",
                &execute::number_to_js_string(*n),
            ))
        }
        _ => {
            return Err(crate::modules::buffer_enc::invalid_arg_type(
                "The \"n\" argument must be a non-negative number".into(),
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
