//! `events` module — `EventEmitter` methods and module exports.
//!
//! Listener state lives in `modules::emitter`; `EventTarget` and the
//! statics live in `modules::event_target`. Every emitter method is
//! a host capability dispatched with the JS receiver.

use std::cell::RefCell;
use std::rc::Rc;

use quench_runtime::execute::{self, VmError};
use quench_runtime::host_api;
use quench_runtime::value::Value;

use crate::host::HostState;
use crate::modules::emitter::{emitter_id, EmitterId, EventEmitter, Listener, EMITTER_ID_PROP};

/// Resolve the emitter for a receiver, throwing Node-style when the
/// receiver is not an emitter at all.
fn expect_emitter(state: &Rc<RefCell<HostState>>, receiver: Option<&Value>) -> Option<EmitterId> {
    let receiver = receiver?;
    let registry = &state.borrow().emitters;
    Some(receiver)
        .and_then(emitter_id)
        .or_else(|| registry.identity(receiver))
        .filter(|id| registry.get(*id).is_some())
}

/// EventEmitter methods are valid during a subclass constructor, before the
/// base `EventEmitter` constructor has run. Allocate the host-side state on
/// first use so JS prototype inheritance and host identity remain aligned.
fn ensure_emitter(state: &Rc<RefCell<HostState>>, receiver: Option<&Value>) -> Option<EmitterId> {
    let receiver = receiver?;
    if let Some(id) = expect_emitter(state, Some(receiver)) {
        return Some(id);
    }
    if !matches!(
        receiver,
        Value::Object(_) | Value::Array(_) | Value::Function(_) | Value::BoundFunction(_)
    ) {
        return None;
    }
    let id = state.borrow_mut().emitters.allocate();
    state
        .borrow_mut()
        .emitters
        .insert(id, Rc::new(RefCell::new(EventEmitter::new())));
    let events = execute::set_property(host_api::object(Vec::new()), "\0prototype", Value::Null);
    let mut updated = execute::set_property(receiver.clone(), "_events", events);
    updated = execute::set_property(updated, EMITTER_ID_PROP, Value::Number(id.0 as f64));
    updated = execute::set_property(updated, "_eventsCount", Value::Number(0.0));
    execute::replace_value(receiver, &updated);
    state.borrow_mut().emitters.bind_identity(&updated, id);
    Some(id)
}

pub fn initialize_emitter(state: &Rc<RefCell<HostState>>, receiver: &Value) -> Result<(), VmError> {
    ensure_emitter(state, Some(receiver))
        .map(|_| ())
        .ok_or_else(|| execute::type_error("receiver is not an EventEmitter"))
}

fn event_name(value: Option<&Value>) -> Result<String, VmError> {
    match value {
        Some(Value::String(name)) => Ok(name.clone()),
        // Node canonicalizes numeric event names through property-key
        // coercion (`on(1, fn)` and `emit('1')` address the same channel).
        Some(Value::Number(number)) => Ok(execute::number_to_js_string(*number)),
        _ => Err(execute::type_error(
            "The \"event\" argument must be of type string or symbol",
        )),
    }
}

fn expect_listener(args: &[Value]) -> Result<Value, VmError> {
    match args.get(1) {
        Some(value) if quench_runtime::is_callable(value) => Ok(value.clone()),
        _ => Err(execute::type_error(
            "The \"listener\" argument must be of type function",
        )),
    }
}

fn add_listener(
    state: &Rc<RefCell<HostState>>,
    receiver: Option<&Value>,
    args: &[Value],
    once: bool,
    prepend: bool,
) -> Result<Value, VmError> {
    let event = event_name(args.first())?;
    let callback = expect_listener(args)?;
    let Some(id) = ensure_emitter(state, receiver) else {
        return Err(execute::type_error("receiver is not an EventEmitter"));
    };
    let Some(emitter) = state.borrow().emitters.get(id) else {
        return Err(execute::type_error("receiver is not an EventEmitter"));
    };
    let (count, max, already_warned) = {
        let mut guard = emitter.borrow_mut();
        let count = guard.add(&event, callback, once, prepend);
        (count, guard.max, guard.warned)
    };
    let limit = max.unwrap_or(state.borrow().emitters.default_max);
    if count > limit && limit > 0 && !already_warned {
        emitter.borrow_mut().warned = true;
        warn_max_listeners(state, &event, count);
    }
    Ok(receiver.cloned().unwrap_or(Value::Undefined))
}

/// Queue a `MaxListenersExceededWarning` process warning, mirroring
/// Node's one-warning-per-emitter behavior.
fn warn_max_listeners(state: &Rc<RefCell<HostState>>, event: &str, count: usize) {
    let message = format!(
        "Possible EventEmitter memory leak detected. {count} {event} listeners added. Use emitter.setMaxListeners() to increase limit"
    );
    let warning = host_api::object(vec![
        (
            "name".to_string(),
            Value::String("MaxListenersExceededWarning".to_string()),
        ),
        ("message".to_string(), Value::String(message)),
    ]);
    let handlers: Vec<Value> = state
        .borrow()
        .process
        .warning_handlers
        .iter()
        .map(|(handler, _)| handler.clone())
        .collect();
    for handler in handlers {
        state
            .borrow_mut()
            .event_loop
            .queue_microtask(handler, vec![warning.clone()]);
    }
}

pub fn method_on(
    state: &Rc<RefCell<HostState>>,
    receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    add_listener(state, receiver, args, false, false)
}

pub fn method_once(
    state: &Rc<RefCell<HostState>>,
    receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    add_listener(state, receiver, args, true, false)
}

pub fn method_prepend_listener(
    state: &Rc<RefCell<HostState>>,
    receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    add_listener(state, receiver, args, false, true)
}

pub fn method_prepend_once_listener(
    state: &Rc<RefCell<HostState>>,
    receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    add_listener(state, receiver, args, true, true)
}

pub fn method_emit(
    state: &Rc<RefCell<HostState>>,
    receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    let Some(receiver) = receiver else {
        return Ok(Value::Boolean(false));
    };
    let Some(id) = ensure_emitter(state, Some(receiver)) else {
        return Ok(Value::Boolean(false));
    };
    let event = match event_name(args.first()) {
        Ok(name) => name,
        Err(_) => return Ok(Value::Boolean(false)),
    };
    let Some(emitter) = state.borrow().emitters.get(id) else {
        return Ok(Value::Boolean(false));
    };
    let snapshot: Vec<Listener> = emitter.borrow().listeners_of(&event).to_vec();
    if snapshot.is_empty() {
        if event == "error" {
            if let Some(result) = route_domain_error(receiver, args.get(1))? {
                return Ok(result);
            }
            return Err(unhandled_error(args.get(1)));
        }
        return Ok(Value::Boolean(false));
    }
    let rest: Vec<Value> = args.get(1..).unwrap_or(&[]).to_vec();
    for listener in &snapshot {
        if listener.once {
            emitter.borrow_mut().remove(&event, &listener.callback);
        }
        execute::call(&listener.callback, receiver, &rest)?;
    }
    Ok(Value::Boolean(true))
}

/// Domain membership is an edge concern: EventEmitter owns the emission, and
/// the attached domain owns unhandled-error policy.
fn route_domain_error(
    receiver: &Value,
    argument: Option<&Value>,
) -> Result<Option<Value>, VmError> {
    let domain = execute::get_property_result(receiver, "domain")?;
    if matches!(domain, Value::Undefined | Value::Null) {
        return Ok(None);
    }
    let handler = execute::get_property_result(&domain, "_handler")?;
    if matches!(handler, Value::Undefined | Value::Null) {
        return Ok(None);
    }
    let original = argument.cloned();
    let error = argument.cloned().unwrap_or_else(|| {
        host_api::object(vec![(
            "message".into(),
            Value::String("Unhandled error.".into()),
        )])
    });
    let error = execute::set_property(error, "domain", domain.clone());
    let error = execute::set_property(error, "domainEmitter", receiver.clone());
    let error = execute::set_property(error, "domainThrown", Value::Boolean(false));
    if let Some(original) = original {
        execute::replace_value(&original, &error);
    }
    execute::call(&handler, &domain, &[error]).map(Some)
}

/// `emit('error')` with no listeners throws the error argument.
fn unhandled_error(arg: Option<&Value>) -> VmError {
    match arg {
        Some(value) if !matches!(value, Value::Undefined) => VmError::Thrown(value.clone()),
        _ => VmError::Thrown(host_api::object(vec![
            ("name".to_string(), Value::String("Error".to_string())),
            (
                "message".to_string(),
                Value::String("Unhandled error.".to_string()),
            ),
            (
                "code".to_string(),
                Value::String("ERR_UNHANDLED_ERROR".to_string()),
            ),
        ])),
    }
}

pub fn method_remove_listener(
    state: &Rc<RefCell<HostState>>,
    receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    let event = event_name(args.first())?;
    let callback = expect_listener(args)?;
    if let Some(id) = expect_emitter(state, receiver) {
        if let Some(emitter) = state.borrow().emitters.get(id) {
            emitter.borrow_mut().remove(&event, &callback);
        }
    }
    Ok(receiver.cloned().unwrap_or(Value::Undefined))
}

pub fn method_remove_all_listeners(
    state: &Rc<RefCell<HostState>>,
    receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    if let Some(id) = expect_emitter(state, receiver) {
        if let Some(emitter) = state.borrow().emitters.get(id) {
            let mut guard = emitter.borrow_mut();
            match args.first() {
                Some(Value::String(event)) => guard.events.retain(|(key, _)| key != event),
                Some(Value::Undefined) | None => guard.events.clear(),
                _ => {
                    return Err(execute::type_error(
                        "The \"event\" argument must be a string",
                    ))
                }
            }
        }
    }
    Ok(receiver.cloned().unwrap_or(Value::Undefined))
}

pub fn method_listeners(
    state: &Rc<RefCell<HostState>>,
    receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    let event = event_name(args.first())?;
    let callbacks = expect_emitter(state, receiver)
        .and_then(|id| state.borrow().emitters.get(id))
        .map(|emitter| {
            emitter
                .borrow()
                .listeners_of(&event)
                .iter()
                .map(|listener| listener.callback.clone())
                .collect()
        })
        .unwrap_or_default();
    Ok(host_api::array(callbacks))
}

pub fn method_event_names(
    state: &Rc<RefCell<HostState>>,
    receiver: Option<&Value>,
    _args: &[Value],
) -> Result<Value, VmError> {
    let names = expect_emitter(state, receiver)
        .and_then(|id| state.borrow().emitters.get(id))
        .map(|emitter| {
            emitter
                .borrow()
                .events
                .iter()
                .map(|(key, _)| Value::String(key.clone()))
                .collect()
        })
        .unwrap_or_default();
    Ok(host_api::array(names))
}

pub fn method_listener_count(
    state: &Rc<RefCell<HostState>>,
    receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    let event = event_name(args.first())?;
    let count = expect_emitter(state, receiver)
        .and_then(|id| state.borrow().emitters.get(id))
        .map(|emitter| {
            let guard = emitter.borrow();
            let list = guard.listeners_of(&event);
            match args.get(1) {
                Some(Value::Undefined) | None => list.len(),
                Some(callback) => list
                    .iter()
                    .filter(|listener| execute::same_value(&listener.callback, callback))
                    .count(),
            }
        })
        .unwrap_or(0);
    Ok(Value::Number(count as f64))
}

pub fn method_set_max_listeners(
    state: &Rc<RefCell<HostState>>,
    receiver: Option<&Value>,
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
    if let Some(id) = expect_emitter(state, receiver) {
        if let Some(emitter) = state.borrow().emitters.get(id) {
            emitter.borrow_mut().max = Some(n);
        }
    }
    Ok(receiver.cloned().unwrap_or(Value::Undefined))
}

pub fn method_get_max_listeners(
    state: &Rc<RefCell<HostState>>,
    receiver: Option<&Value>,
    _args: &[Value],
) -> Result<Value, VmError> {
    let max = expect_emitter(state, receiver)
        .and_then(|id| state.borrow().emitters.get(id))
        .and_then(|emitter| emitter.borrow().max)
        .unwrap_or_else(|| state.borrow().emitters.default_max);
    Ok(Value::Number(max as f64))
}

pub fn new_emitter(state: &Rc<RefCell<HostState>>, args: &[Value]) -> Result<Value, VmError> {
    let id = state.borrow_mut().emitters.allocate();
    let emitter = Rc::new(RefCell::new(EventEmitter::new()));
    state.borrow_mut().emitters.insert(id, emitter);
    let id_value = Value::Number(id.0 as f64);
    let mut object =
        crate::host::namespace_object_from_pairs(vec![(EMITTER_ID_PROP.to_string(), id_value)]);
    let capture = args
        .first()
        .filter(|value| matches!(value, Value::Object(_) | Value::ObjectAlias(_)))
        .and_then(|options| execute::get_property_result(options, "captureRejections").ok())
        .is_some_and(|value| matches!(value, Value::Boolean(true)));
    if capture {
        object = execute::set_property(object, "Symbol.kCapture\0quench", Value::Boolean(true));
    }
    install_emitter_props(object)
}

/// Build a fresh EventEmitter-backed object with all standard emitter
/// methods installed. Shared by `net` for its server/socket objects.
pub fn new_emitter_object(state: &Rc<RefCell<HostState>>) -> Result<Value, VmError> {
    new_emitter(state, &[])
}

fn install_emitter_props(mut object: Value) -> Result<Value, VmError> {
    for (key, value) in emitter_props() {
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

fn emitter_props() -> Vec<(&'static str, Value)> {
    vec![
        ("on", cap("events:on", 0x0102)),
        ("addListener", cap("events:on", 0x0102)),
        ("once", cap("events:once", 0x0105)),
        ("emit", cap("events:emit", 0x0103)),
        ("removeListener", cap("events:removeListener", 0x0106)),
        ("off", cap("events:removeListener", 0x0106)),
        (
            "removeAllListeners",
            cap("events:removeAllListeners", 0x0107),
        ),
        ("listeners", cap("events:listeners", 0x0108)),
        ("eventNames", cap("events:eventNames", 0x0109)),
        ("listenerCount", cap("events:listenerCount", 0x010A)),
        ("prependListener", cap("events:prependListener", 0x010B)),
        (
            "prependOnceListener",
            cap("events:prependOnceListener", 0x010C),
        ),
        ("setMaxListeners", cap("events:setMaxListeners", 0x010D)),
        ("getMaxListeners", cap("events:getMaxListeners", 0x010E)),
    ]
}

fn cap(name: &'static str, id: u16) -> Value {
    crate::host::capability(crate::registry::NodeSpec::new(name, id))
}

pub fn from(_state: &Rc<RefCell<HostState>>, args: &[Value]) -> Result<Value, VmError> {
    let target = args.first().cloned().unwrap_or(Value::Undefined);
    let out = vec![
        ("on".to_string(), cap("events:on", 0x0102)),
        ("emit".to_string(), cap("events:emit", 0x0103)),
        ("target".to_string(), target),
    ];
    Ok(host_api::object(out))
}

/// Shared thunk used by `setTimeout` / `setImmediate` to push a
/// callback into the host event loop.
pub fn enqueue_callback(state: &Rc<RefCell<HostState>>, cb: Value, args: Vec<Value>) {
    state.borrow_mut().event_loop.queue_immediate(cb, args);
}

/// `require('events')` is the `EventEmitter` constructor itself, with
/// the statics attached — mirroring Node's `module.exports =
/// EventEmitter; EventEmitter.EventEmitter = EventEmitter`.
pub fn build() -> Value {
    let value = crate::host::capability(crate::registry::SPEC_EVENTS_NEW);
    let once = eval_function(
        r#"(emitter, event, options) => {
          if (options !== undefined &&
              (options === null || typeof options !== "object")) {
            const error = new TypeError("The options argument must be an object");
            error.code = "ERR_INVALID_ARG_TYPE";
            return Promise.reject(error);
          }
          options ||= {};
          if (options.signal !== undefined &&
              (options.signal === null ||
               typeof options.signal !== "object" ||
               typeof options.signal.addEventListener !== "function")) {
            const error = new TypeError("The signal option must be an AbortSignal");
            error.code = "ERR_INVALID_ARG_TYPE";
            return Promise.reject(error);
          }
          if (typeof emitter?.once !== "function" &&
              typeof emitter?.addEventListener !== "function") {
            const error = new TypeError("The emitter must be an EventEmitter or EventTarget");
            error.code = "ERR_INVALID_ARG_TYPE";
            return Promise.reject(error);
          }
          return new Promise((resolve, reject) => {
            const target = typeof emitter.once === "function";
            const remove = () => {
              if (target) {
                emitter.removeListener?.(event, onEvent);
                if (event !== "error") emitter.removeListener?.("error", onError);
              } else emitter.removeEventListener?.(event, onEvent);
              options.signal?.removeEventListener?.("abort", onAbort);
            };
            const onEvent = (...args) => { remove(); resolve(args); };
            const onError = (error) => { remove(); reject(error); };
            const onAbort = () => {
              remove();
              reject(Object.assign(new Error("The operation was aborted"), {
                name: "AbortError", code: "ABORT_ERR"
              }));
            };
            if (target) {
              emitter.once(event, onEvent);
              if (event !== "error") emitter.once("error", onError);
            } else emitter.addEventListener(event, onEvent, { once: true });
            if (options.signal?.aborted) onAbort();
            else options.signal?.addEventListener?.("abort", onAbort, { once: true });
            queueMicrotask(() => { if (options.signal?.aborted) onAbort(); });
          });
        }"#,
    )
    .unwrap_or(Value::Undefined);
    let props: Vec<(String, Value)> = vec![
        ("EventEmitter".to_string(), value.clone()),
        ("defaultMaxListeners".to_string(), Value::Number(10.0)),
        ("once".to_string(), once),
        (
            "getMaxListeners".to_string(),
            cap("events:getMaxListeners:static", 0x0112),
        ),
        (
            "setMaxListeners".to_string(),
            cap("events:setMaxListeners:static", 0x010F),
        ),
        (
            "getEventListeners".to_string(),
            cap("events:getEventListeners", 0x0110),
        ),
        (
            "listenerCount".to_string(),
            cap("events:listenerCount:static", 0x0111),
        ),
    ];
    for (key, property) in props {
        let _ = execute::set_callable_property(&value, &key, property);
    }
    value
}

fn eval_function(source: &str) -> Result<Value, VmError> {
    let program = quench_runtime::reduce::reduce_global_script_source(source)
        .map_err(|errors| VmError::EvalError(errors.join("; ")))?;
    let context = quench_runtime::vm::current_context();
    let mut registers = quench_runtime::register_file::RegisterFile::new();
    quench_runtime::vm::execute_code_in_place_context(program.code(), &mut registers, &context)
}
