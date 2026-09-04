//! `events` module — `EventEmitter` methods and module exports.
//!
//! Listener state lives in `modules::emitter`; `EventTarget` and the
//! statics live in `modules::event_target`. Every emitter method is
//! a host capability dispatched with the JS receiver.

use std::cell::Cell;
use std::cell::RefCell;
use std::rc::Rc;

use quench_runtime::execute::{self, VmError};
use quench_runtime::host_api;
use quench_runtime::value::Value;

use crate::host::HostState;
use crate::modules::emitter::{emitter_id, EmitterId, EventEmitter, Listener, EMITTER_ID_PROP};

const CAPTURE_SUPPRESSED_PROP: &str = "\0quench:events:capture-suppressed";

thread_local! {
    static CAPTURE_REJECTIONS: Cell<bool> = const { Cell::new(false) };
    static DEFAULT_MAX_LISTENERS: Cell<usize> = const { Cell::new(10) };
    static CAPTURE_SUPPRESSION_DEPTH: Cell<usize> = const { Cell::new(0) };
}

pub fn default_max_get(_state: &Rc<RefCell<HostState>>, _args: &[Value]) -> Result<Value, VmError> {
    Ok(Value::Number(DEFAULT_MAX_LISTENERS.with(Cell::get) as f64))
}

pub fn default_max_set(state: &Rc<RefCell<HostState>>, args: &[Value]) -> Result<Value, VmError> {
    let n = validate_max_listeners(args.first())?;
    DEFAULT_MAX_LISTENERS.with(|value| value.set(n));
    state.borrow_mut().emitters.default_max = n;
    Ok(Value::Undefined)
}

fn validate_max_listeners(value: Option<&Value>) -> Result<usize, VmError> {
    match value {
        Some(Value::Number(n)) if *n >= 0.0 && n.is_finite() => Ok(*n as usize),
        Some(Value::Number(n)) => Err(crate::modules::buffer_enc::out_of_range(
            "defaultMaxListeners",
            "a non-negative number",
            &execute::number_to_js_string(*n),
        )),
        _ => Err(crate::modules::buffer_enc::invalid_arg_type(
            "The \"defaultMaxListeners\" property must be a non-negative number".into(),
        )),
    }
}

pub fn capture_rejections_get(
    _state: &Rc<RefCell<HostState>>,
    _args: &[Value],
) -> Result<Value, VmError> {
    Ok(Value::Boolean(CAPTURE_REJECTIONS.with(Cell::get)))
}

pub fn capture_rejections_set(
    _state: &Rc<RefCell<HostState>>,
    args: &[Value],
) -> Result<Value, VmError> {
    let value = args.first().cloned().unwrap_or(Value::Undefined);
    if !matches!(value, Value::Boolean(_)) {
        return Err(crate::modules::buffer_enc::invalid_arg_type(format!(
            "The \"EventEmitter.captureRejections\" property must be of type boolean.{}",
            crate::modules::util::invalid_arg_received(&value)
        )));
    }
    CAPTURE_REJECTIONS.with(|capture| capture.set(matches!(value, Value::Boolean(true))));
    Ok(Value::Undefined)
}

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
        Value::Object(_)
            | Value::ObjectAlias(_)
            | Value::Array(_)
            | Value::Function(_)
            | Value::BoundFunction(_)
    ) {
        return None;
    }
    let id = state.borrow_mut().emitters.allocate();
    let emitter = Rc::new(RefCell::new(EventEmitter::new()));
    emitter.borrow_mut().capture_rejections = CAPTURE_REJECTIONS.with(Cell::get);
    state.borrow_mut().emitters.insert(id, emitter);
    let events = execute::set_property(host_api::object(Vec::new()), "\0prototype", Value::Null);
    if matches!(receiver, Value::ObjectAlias(_)) {
        execute::set_property_in_place(receiver, "_events", events);
        execute::set_property_in_place(receiver, EMITTER_ID_PROP, Value::Number(id.0 as f64));
        execute::set_property_in_place(receiver, "_eventsCount", Value::Number(0.0));
        execute::set_property_in_place(receiver, "domain", Value::Undefined);
    } else {
        let mut updated = execute::set_property(receiver.clone(), "_events", events);
        updated = execute::set_property(updated, EMITTER_ID_PROP, Value::Number(id.0 as f64));
        updated = execute::set_property(updated, "_eventsCount", Value::Number(0.0));
        updated = execute::set_property(updated, "domain", Value::Undefined);
        execute::replace_value(receiver, &updated);
    }
    state.borrow_mut().emitters.bind_identity(receiver, id);
    Some(id)
}

pub fn initialize_emitter(state: &Rc<RefCell<HostState>>, receiver: &Value) -> Result<(), VmError> {
    let Some(id) = expect_emitter(state, Some(receiver)) else {
        return ensure_emitter(state, Some(receiver))
            .map(|_| ())
            .ok_or_else(|| execute::type_error("receiver is not an EventEmitter"));
    };
    if let Some(emitter) = state.borrow().emitters.get(id) {
        let capture = CAPTURE_REJECTIONS.with(Cell::get);
        *emitter.borrow_mut() = EventEmitter::new();
        emitter.borrow_mut().capture_rejections = capture;
    }
    reset_emitter_properties(receiver);
    Ok(())
}

fn reset_emitter_properties(receiver: &Value) {
    let events = execute::set_property(host_api::object(Vec::new()), "\0prototype", Value::Null);
    let updated = execute::set_property(receiver.clone(), "_events", events);
    let updated = execute::set_property(updated, "_eventsCount", Value::Number(0.0));
    let updated = execute::set_property(updated, "domain", Value::Undefined);
    execute::replace_value(receiver, &updated);
}

fn event_name(value: Option<&Value>) -> Result<String, VmError> {
    match value {
        Some(Value::String(name)) => Ok(name.clone()),
        Some(Value::Null) => Ok("null".into()),
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
        Some(value) => Err(crate::modules::buffer_enc::invalid_arg_type(format!(
            "The \"listener\" argument must be of type function.{}",
            crate::modules::util::invalid_arg_received(value)
        ))),
        None => Err(crate::modules::buffer_enc::invalid_arg_type(
            "The \"listener\" argument must be of type function. Received undefined".into(),
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
    let event_value = args.first().cloned().unwrap_or(Value::Undefined);
    let event = event_name(args.first())?;
    let callback = expect_listener(args)?;
    let Some(id) = ensure_emitter(state, receiver) else {
        return Err(execute::type_error("receiver is not an EventEmitter"));
    };
    if let Some(receiver) = receiver {
        if let Some(id) = crate::modules::net::net_id(receiver) {
            if let Some(socket) = state.borrow().net.sockets.get(&id).cloned() {
                let alpn = execute::get_property(
                    &socket.borrow().js,
                    crate::modules::tls::TLS_NEGOTIATED_ALPN_PROP,
                );
                if !matches!(alpn, Value::Undefined) {
                    execute::set_property_in_place(receiver, "alpnProtocol", alpn);
                }
            }
        }
    }
    let Some(emitter) = state.borrow().emitters.get(id) else {
        return Err(execute::type_error("receiver is not an EventEmitter"));
    };
    if event != "newListener" {
        let _ = method_emit(
            state,
            receiver,
            &[
                Value::String("newListener".into()),
                Value::String(event.clone()),
                callback.clone(),
            ],
        )?;
    }
    let already_warned = receiver.is_some_and(|value| event_is_warned(value, &event));
    let process_scope = state.borrow().cluster.process_scope();
    let (count, max) = {
        let mut guard = emitter.borrow_mut();
        let count = guard.add(&event, callback.clone(), once, prepend, process_scope);
        (count, guard.max)
    };
    if let Some(receiver) = receiver {
        if let Ok(events) = execute::get_property_result(receiver, "_events") {
            let callbacks = emitter
                .borrow()
                .listeners_for_scope(&event, process_scope)
                .iter()
                .map(|listener| listener.callback.clone())
                .collect::<Vec<_>>();
            let value = if callbacks.len() == 1 {
                callbacks[0].clone()
            } else {
                host_api::array(callbacks)
            };
            let updated = execute::set_property(events.clone(), &event, value);
            execute::replace_value(&events, &updated);
        }
        if event == "message"
            && matches!(
                execute::get_property(receiver, "\0childForkIpc"),
                Value::Boolean(true)
            )
        {
            let pending = execute::get_property(receiver, "\0childPendingMessages");
            execute::set_property_in_place(
                receiver,
                "\0childPendingMessages",
                host_api::array(Vec::new()),
            );
            if let Value::Array(ref values) = pending {
                for index in 0..values.logical_len() {
                    let entry = execute::get_property(&pending, &index.to_string());
                    let args = match entry {
                        Value::Array(ref array) => (0..array.logical_len())
                            .map(|arg| execute::get_property(&entry, &arg.to_string()))
                            .collect(),
                        value => vec![value],
                    };
                    state.borrow().event_loop.queue_microtask_with_receiver(
                        callback.clone(),
                        args,
                        receiver.clone(),
                    );
                }
            }
        }
    }
    let limit = max.unwrap_or(state.borrow().emitters.default_max);
    if count > limit && limit > 0 && !already_warned {
        mark_event_warned(receiver.expect("validated emitter"), &event);
        warn_max_listeners(
            state,
            receiver.expect("validated emitter"),
            &event,
            &event_value,
            count,
            limit,
        );
    }
    // Listener registration may publish a copy-on-write representative while
    // initializing the emitter. Return that live identity so fluent calls
    // (`socket.on(...).on(...)`) retain host-owned connection metadata.
    let result = receiver
        .map(execute::canonical_value)
        .unwrap_or(Value::Undefined);
    Ok(result)
}

fn mark_event_warned(receiver: &Value, event: &str) {
    let Ok(events) = execute::get_property_result(receiver, "_events") else {
        return;
    };
    let current = execute::get_property(&events, event);
    if matches!(current, Value::Array(_)) {
        let updated = execute::set_property(current.clone(), "warned", Value::Boolean(true));
        execute::replace_value(&current, &updated);
    }
}

fn event_is_warned(receiver: &Value, event: &str) -> bool {
    let Ok(events) = execute::get_property_result(receiver, "_events") else {
        return false;
    };
    let current = execute::get_property(&events, event);
    matches!(
        execute::get_property(&current, "warned"),
        Value::Boolean(true)
    )
}

fn sync_event_property(receiver: Option<&Value>, event: &str, listeners: &[Listener]) {
    let Some(receiver) = receiver else { return };
    let Ok(events) = execute::get_property_result(receiver, "_events") else {
        return;
    };
    let updated = if listeners.is_empty() {
        execute::delete_property(events.clone(), event).0
    } else if listeners.len() == 1 {
        execute::set_property(events.clone(), event, listeners[0].callback.clone())
    } else {
        execute::set_property(
            events.clone(),
            event,
            host_api::array(
                listeners
                    .iter()
                    .map(|listener| listener.callback.clone())
                    .collect(),
            ),
        )
    };
    execute::replace_value(&events, &updated);
}

/// Queue a `MaxListenersExceededWarning` process warning, mirroring
/// Node's one-warning-per-emitter behavior.
fn warn_max_listeners(
    state: &Rc<RefCell<HostState>>,
    emitter: &Value,
    event: &str,
    event_value: &Value,
    count: usize,
    limit: usize,
) {
    let label = match execute::get_property(emitter, "constructor") {
        constructor if quench_runtime::is_callable(&constructor) => {
            match execute::get_property(&constructor, "name") {
                Value::String(name) if !name.is_empty() && name != "Object" => {
                    format!("[{name}]")
                }
                _ => "[EventEmitter]".into(),
            }
        }
        _ => "[EventEmitter]".into(),
    };
    let display = execute::to_js_string_explicit(event_value).unwrap_or_else(|_| event.to_string());
    let message = format!(
        "Possible EventEmitter memory leak detected. {count} {display} listeners added to {label}. MaxListeners is {limit}. Use emitter.setMaxListeners() to increase limit"
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
    let warning = execute::set_property(warning, "emitter", emitter.clone());
    let warning = execute::set_property(warning, "count", Value::Number(count as f64));
    let warning = execute::set_property(warning, "type", event_value.clone());
    let process_emit = crate::host::capability(crate::registry::SPEC_PROCESS_EMIT);
    state
        .borrow_mut()
        .event_loop
        .queue_microtask(process_emit, vec![Value::String("warning".into()), warning]);
}

pub fn method_on(
    state: &Rc<RefCell<HostState>>,
    receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    let result = add_listener(state, receiver, args, false, false)?;
    if matches!(args.first(), Some(Value::String(event)) if event == "keylog")
        && receiver.is_some_and(|value| {
            matches!(
                execute::get_property(value, "\0quench:http:agent"),
                Value::Boolean(true)
            )
        })
    {
        if let Some(listener) = args.get(1) {
            crate::modules::http_client::agent_keylog_attach(
                state,
                receiver.expect("validated agent"),
                listener,
            )?;
        }
    }
    Ok(result)
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
    // Clone the emitter handle while the host is immutably borrowed, then
    // release that borrow before invoking user listeners. Listener callbacks
    // may re-enter host APIs (including diagnostics channels), and retaining
    // the map lookup's RefCell borrow across those calls would panic.
    let emitter = { state.borrow().emitters.get(id) };
    let Some(emitter) = emitter else {
        return Ok(Value::Boolean(false));
    };
    let process_scope = state.borrow().cluster.process_scope();
    if event == "error" {
        let monitor = emitter
            .borrow()
            .listeners_for_scope("Symbol.for.events.errorMonitor\0", process_scope);
        for listener in monitor {
            execute::call(&listener.callback, receiver, args.get(1..).unwrap_or(&[]))?;
        }
    }
    let capture_suppressed = CAPTURE_SUPPRESSION_DEPTH.with(|depth| depth.get() > 0)
        || matches!(
            execute::get_property(receiver, CAPTURE_SUPPRESSED_PROP),
            Value::Boolean(true)
        );
    let capture_rejections = emitter.borrow().capture_rejections && !capture_suppressed;
    let snapshot = emitter.borrow().listeners_for_scope(&event, process_scope);
    if snapshot.is_empty() {
        if event == "error" {
            if CAPTURE_SUPPRESSION_DEPTH.with(|depth| depth.get() > 0)
                || matches!(
                    execute::get_property(receiver, CAPTURE_SUPPRESSED_PROP),
                    Value::Boolean(true)
                )
            {
                let emitted = crate::modules::process::emit(
                    state,
                    &[
                        Value::String("uncaughtException".into()),
                        args.get(1).cloned().unwrap_or(Value::Undefined),
                        Value::String("unhandledRejection".into()),
                    ],
                )?;
                if execute::is_truthy(&emitted) {
                    return Ok(Value::Boolean(true));
                }
            }
            if let Some(result) = route_domain_error(state, receiver, args.get(1))? {
                return Ok(result);
            }
            return Err(unhandled_error(args.get(1)));
        }
        return Ok(Value::Boolean(false));
    }
    if event == "error"
        && snapshot.iter().all(|listener| {
            matches!(
                execute::get_property_result(&listener.callback, "__quenchInternal"),
                Ok(Value::Boolean(true))
            )
        })
    {
        // Internal lifecycle listeners (for example stream auto-destroy) run
        // before deciding whether the error remains observable.  An error
        // arriving on a live stream is consumed by auto-destroy; one emitted
        // after explicit destruction still follows Node's unhandled path.
        let owner = execute::get_property(receiver, "__quenchOwner");
        let initially_destroyed = matches!(
            execute::get_property(receiver, "destroyed"),
            Value::Boolean(true)
        ) || matches!(
            execute::get_property(&owner, "destroyed"),
            Value::Boolean(true)
        );
        for listener in &snapshot {
            let _ = execute::call(&listener.callback, receiver, args.get(1..).unwrap_or(&[]));
        }
        if initially_destroyed {
            return Err(unhandled_error(args.get(1)));
        }
        return Ok(Value::Boolean(true));
    }
    let rest: Vec<Value> = args.get(1..).unwrap_or(&[]).to_vec();
    for value in &rest {
        if let Some(id) = crate::modules::net::net_id(value) {
            if let Some(socket) = state.borrow().net.sockets.get(&id).cloned() {
                let alpn = execute::get_property(
                    &socket.borrow().js,
                    crate::modules::tls::TLS_NEGOTIATED_ALPN_PROP,
                );
                if !matches!(alpn, Value::Undefined) {
                    execute::set_property_in_place(value, "alpnProtocol", alpn);
                }
            }
        }
    }
    for listener in &snapshot {
        if listener.once
            && !emitter
                .borrow()
                .listeners_of(&event)
                .iter()
                .any(|current| execute::same_value(&current.callback, &listener.callback))
        {
            continue;
        }
        if listener.once {
            let removed =
                emitter
                    .borrow_mut()
                    .remove_for_scope(&event, &listener.callback, process_scope);
            if removed && event != "removeListener" {
                let _ = method_emit(
                    state,
                    Some(receiver),
                    &[
                        Value::String("removeListener".into()),
                        Value::String(event.clone()),
                        listener.callback.clone(),
                    ],
                )?;
            }
        }
        let result = match execute::call(&listener.callback, receiver, &rest) {
            Ok(result) => result,
            Err(VmError::Thrown(reason)) if event == "error" => {
                crate::modules::process::emit(
                    state,
                    &[
                        Value::String("uncaughtException".into()),
                        reason,
                        Value::String("uncaughtException".into()),
                    ],
                )?;
                continue;
            }
            Err(error) => return Err(error),
        };
        if matches!(result, Value::Promise(_)) {
            if capture_rejections || capture_suppressed {
                attach_rejection_handler(state, receiver, &event, &rest, &result)?;
            } else {
                attach_unhandled_rejection(&result)?;
            }
        } else if capture_rejections
            && matches!(
                result,
                Value::Object(_)
                    | Value::ObjectAlias(_)
                    | Value::Function(_)
                    | Value::BoundFunction(_)
            )
        {
            attach_rejection_handler(state, receiver, &event, &rest, &result)?;
        }
    }
    Ok(Value::Boolean(true))
}

fn attach_rejection_handler(
    state: &Rc<RefCell<HostState>>,
    receiver: &Value,
    event: &str,
    arguments: &[Value],
    promise: &Value,
) -> Result<(), VmError> {
    let handler = eval_function(
        r#"(emitter, event, arguments, error) => {
          // Stream capture rejections share one emitter with lifecycle
          // listeners. Once the first rejection destroys its owner, later
          // drain/data rejections are no longer observable in Node.
          if (emitter.__quenchOwner?.destroyed === true) return;
          const rejection = emitter[Symbol.for('nodejs.rejection')];
          if (typeof rejection === 'function') {
            try {
              const result = rejection.call(emitter, error, event, ...arguments);
              if (result && typeof result.then === 'function') {
                result.then(undefined, (reason) => process.emit('unhandledRejection', reason));
              }
            } catch (reason) {
              process.emit('unhandledRejection', reason);
            }
          } else {
            if (event === 'error') {
              process.emit('unhandledRejection', error);
            } else {
              const key = "\0quench:events:capture-suppressed";
              emitter[key] = true;
              try { emitter.emit('error', error); }
              finally { delete emitter[key]; }
            }
          }
        }"#,
    )?;
    let bind = execute::get_property_result(&handler, "bind")?;
    let bound = execute::call(
        &bind,
        &handler,
        &[
            Value::Undefined,
            receiver.clone(),
            Value::String(event.to_string()),
            host_api::array(arguments.to_vec()),
        ],
    )?;
    let then = match execute::get_property_result(promise, "then") {
        Ok(then) => then,
        Err(error) => {
            if let VmError::Thrown(reason) = error {
                let _ = emit_error_without_capture(state, receiver, reason)?;
            }
            return Ok(());
        }
    };
    if !quench_runtime::is_callable(&then) {
        return Ok(());
    }
    let rejection = execute::get_property(receiver, "Symbol.for.nodejs.rejection\0");
    let result = if quench_runtime::is_callable(&rejection) {
        execute::call(&then, promise, &[Value::Undefined, bound])
    } else {
        with_capture_disabled(state, receiver, || {
            execute::call(&then, promise, &[Value::Undefined, bound])
        })
    };
    match result {
        Ok(Value::Promise(_child)) => {}
        Err(VmError::Thrown(reason)) => {
            let _ = emit_error_without_capture(state, receiver, reason)?;
        }
        _ => {}
    }
    Ok(())
}

/// Node disables rejection capture while routing a rejection to the error
/// channel.  This makes an async `error` listener terminal instead of
/// recursively capturing its own rejected promise.
fn emit_error_without_capture(
    state: &Rc<RefCell<HostState>>,
    receiver: &Value,
    reason: Value,
) -> Result<Value, VmError> {
    with_capture_disabled(state, receiver, || {
        method_emit(
            state,
            Some(receiver),
            &[Value::String("error".into()), reason],
        )
    })
}

fn with_capture_disabled<T>(
    state: &Rc<RefCell<HostState>>,
    receiver: &Value,
    operation: impl FnOnce() -> T,
) -> T {
    let id = ensure_emitter(state, Some(receiver));
    let emitter = id.and_then(|id| state.borrow().emitters.get(id));
    let previous = emitter.as_ref().map(|emitter| {
        let mut guard = emitter.borrow_mut();
        let previous = guard.capture_rejections;
        guard.capture_rejections = false;
        previous
    });
    CAPTURE_SUPPRESSION_DEPTH.with(|depth| depth.set(depth.get() + 1));
    let result = operation();
    CAPTURE_SUPPRESSION_DEPTH.with(|depth| depth.set(depth.get().saturating_sub(1)));
    if let (Some(emitter), Some(previous)) = (emitter, previous) {
        emitter.borrow_mut().capture_rejections = previous;
    }
    result
}

fn attach_unhandled_rejection(promise: &Value) -> Result<(), VmError> {
    let handler = eval_function("(error) => process.emit('unhandledRejection', error)")?;
    let then = execute::get_property_result(promise, "then")?;
    if quench_runtime::is_callable(&then) {
        let _ = execute::call(&then, promise, &[Value::Undefined, handler])?;
    }
    Ok(())
}

/// Domain membership is an edge concern: EventEmitter owns the emission, and
/// the attached domain owns unhandled-error policy.
fn route_domain_error(
    state: &Rc<RefCell<HostState>>,
    receiver: &Value,
    argument: Option<&Value>,
) -> Result<Option<Value>, VmError> {
    let domain = execute::get_property_result(receiver, "domain")?;
    if matches!(domain, Value::Undefined | Value::Null) {
        return Ok(None);
    }
    let handler = crate::modules::domain::error_handler(state, &domain)
        .or_else(|| execute::get_property_result(&domain, "_handler").ok());
    if handler.is_none() || matches!(handler, Some(Value::Undefined | Value::Null)) {
        return Ok(None);
    }
    let original = argument.cloned().filter(|value| {
        !matches!(
            value,
            Value::Null | Value::Undefined | Value::Boolean(false)
        )
    });
    let error = original.clone().unwrap_or_else(|| {
        quench_runtime::builtins::error(
            quench_runtime::ops::Builtin::Error,
            &[Value::String("Unhandled error.".into())],
        )
    });
    let error = execute::set_property(error, "domain", domain.clone());
    let error = execute::set_property(error, "domainEmitter", receiver.clone());
    let error = execute::set_property(error, "domainThrown", Value::Boolean(false));
    if let Some(original) = original {
        execute::replace_value(&original, &error);
    }
    match crate::modules::domain::call_error_handler(
        state,
        &domain,
        handler.as_ref().expect("checked handler"),
        &error,
    ) {
        Ok(value) => Ok(Some(value)),
        Err(VmError::Thrown(reason)) => {
            let reason =
                execute::set_property(reason, crate::modules::domain::HANDLER_DOMAIN, domain);
            Err(VmError::Thrown(reason))
        }
        Err(error) => Err(error),
    }
}

/// `emit('error')` with no listeners throws the error argument.
fn unhandled_error(arg: Option<&Value>) -> VmError {
    if let Some(value) = arg.filter(|value| !matches!(value, Value::Undefined)) {
        if is_error(value) {
            return VmError::Thrown(value.clone());
        }
        let custom = execute::get_property_result(value, "Symbol.for.nodejs.util.inspect.custom\0")
            .ok()
            .filter(quench_runtime::is_callable)
            .or_else(|| execute::get_property_result(value, "undefined").ok());
        let rendered = if quench_runtime::is_callable(custom.as_ref().unwrap_or(&Value::Undefined))
        {
            match execute::call(custom.as_ref().unwrap(), value, &[]) {
                Ok(rendered) => crate::modules::util::inspect(&rendered),
                Err(_) => "[object Object]".into(),
            }
        } else {
            crate::modules::util::inspect(value)
        };
        return VmError::Thrown(host_api::object(vec![
            ("name".to_string(), Value::String("Error".to_string())),
            (
                "message".to_string(),
                Value::String(format!("Unhandled error. ({rendered})")),
            ),
            (
                "code".to_string(),
                Value::String("ERR_UNHANDLED_ERROR".to_string()),
            ),
        ]));
    }
    VmError::Thrown(host_api::object(vec![
        ("name".to_string(), Value::String("Error".to_string())),
        (
            "message".to_string(),
            Value::String("Unhandled error.".to_string()),
        ),
        (
            "code".to_string(),
            Value::String("ERR_UNHANDLED_ERROR".to_string()),
        ),
    ]))
}

fn is_error(value: &Value) -> bool {
    matches!(
        execute::get_property_result(value, "\0error_slot"),
        Ok(Value::Boolean(true))
    )
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
            let removed = emitter.borrow_mut().remove(&event, &callback);
            if removed {
                let remaining = emitter.borrow().listeners_of(&event).to_vec();
                sync_event_property(receiver, &event, &remaining);
            }
            if removed && event != "removeListener" {
                let _ = method_emit(
                    state,
                    receiver,
                    &[
                        Value::String("removeListener".into()),
                        Value::String(event.clone()),
                        callback.clone(),
                    ],
                )?;
            }
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
            let target = match args.first() {
                Some(Value::String(event)) => Some(event.clone()),
                Some(Value::Undefined) | None => None,
                _ => {
                    return Err(execute::type_error(
                        "The \"event\" argument must be a string",
                    ))
                }
            };
            if let Some(event) = target {
                let removed = {
                    let mut guard = emitter.borrow_mut();
                    guard
                        .events
                        .iter()
                        .find(|(key, _)| key == &event)
                        .map(|(_, listeners)| listeners.clone())
                        .unwrap_or_default()
                };
                if !removed.is_empty() {
                    for listener in removed.into_iter().rev() {
                        if emitter.borrow_mut().remove(&event, &listener.callback) {
                            let remaining = emitter.borrow().listeners_of(&event).to_vec();
                            sync_event_property(receiver, &event, &remaining);
                            let _ = method_emit(
                                state,
                                receiver,
                                &[
                                    Value::String("removeListener".into()),
                                    Value::String(event.clone()),
                                    listener.callback,
                                ],
                            )?;
                        }
                    }
                }
            } else {
                let names = emitter
                    .borrow()
                    .events
                    .iter()
                    .map(|(name, _)| name.clone())
                    .filter(|name| name != "removeListener")
                    .collect::<Vec<_>>();
                for name in names {
                    method_remove_all_listeners(state, receiver, &[Value::String(name)])?;
                }
                let removed = emitter.borrow().listeners_of("removeListener").to_vec();
                for listener in removed.into_iter().rev() {
                    let callback = listener.callback;
                    if emitter.borrow_mut().remove("removeListener", &callback) {
                        let remaining = emitter.borrow().listeners_of("removeListener").to_vec();
                        sync_event_property(receiver, "removeListener", &remaining);
                        let _ = method_emit(
                            state,
                            receiver,
                            &[
                                Value::String("removeListener".into()),
                                Value::String("removeListener".into()),
                                callback,
                            ],
                        )?;
                    }
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
    let Some(first) = args.first() else {
        return Ok(host_api::array(Vec::new()));
    };
    let event = event_name(Some(first))?;
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

pub fn method_raw_listeners(
    state: &Rc<RefCell<HostState>>,
    receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    let Some(first) = args.first() else {
        return Ok(host_api::array(Vec::new()));
    };
    let event = event_name(Some(first))?;
    let callbacks = expect_emitter(state, receiver)
        .and_then(|id| state.borrow().emitters.get(id))
        .map(|emitter| {
            emitter
                .borrow()
                .listeners_of(&event)
                .iter()
                .map(|listener| {
                    if !listener.once {
                        return listener.callback.clone();
                    }
                    let wrapper = eval_function(
                        "(emitter, event, listener) => { emitter.removeListener(event, listener); return listener(); }",
                    )
                        .and_then(|wrapper| {
                            let bind = execute::get_property_result(&wrapper, "bind")?;
                            execute::call(
                                &bind,
                                &wrapper,
                                &[
                                    Value::Undefined,
                                    receiver.cloned().unwrap_or(Value::Undefined),
                                    Value::String(event.clone()),
                                    listener.callback.clone(),
                                ],
                            )
                        })
                        .unwrap_or(Value::Undefined);
                    let updated = execute::set_property(
                        wrapper.clone(),
                        "listener",
                        listener.callback.clone(),
                    );
                    execute::replace_value(&wrapper, &updated);
                    wrapper
                })
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
    // The HTTP parser is an internal socket consumer.  Once it reports a
    // parse error Node detaches that consumer before invoking user handlers,
    // while the socket's ordinary lifecycle listeners remain observable.
    // Keep that transition as state rather than mutating the emitter during
    // error delivery (which could drop the socket's end/close progression).
    if event == "data" {
        let parser_failed = receiver
            .and_then(crate::modules::net::net_id)
            .and_then(|socket_id| {
                let guard = state.borrow();
                if guard.http.idle_sockets.contains(&socket_id) {
                    return Some(true);
                }
                let client_id = guard.http.clients.get(&socket_id)?;
                Some(guard.http.clientreqs.get(client_id)?.parse_error)
            })
            .unwrap_or(false);
        if parser_failed {
            return Ok(Value::Number(0.0));
        }
    }
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
    let n = validate_max_listeners(args.first())?;
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
        .unwrap_or_else(|| DEFAULT_MAX_LISTENERS.with(Cell::get));
    Ok(Value::Number(max as f64))
}

pub fn new_emitter(state: &Rc<RefCell<HostState>>, args: &[Value]) -> Result<Value, VmError> {
    let id = state.borrow_mut().emitters.allocate();
    let emitter = Rc::new(RefCell::new(EventEmitter::new()));
    state.borrow_mut().emitters.default_max = DEFAULT_MAX_LISTENERS.with(Cell::get);
    state.borrow_mut().emitters.insert(id, emitter);
    let id_value = Value::Number(id.0 as f64);
    let mut object =
        crate::host::namespace_object_from_pairs(vec![(EMITTER_ID_PROP.to_string(), id_value)]);
    let events = execute::set_property(host_api::object(Vec::new()), "\0prototype", Value::Null);
    object = execute::set_property(object, "_events", events);
    object = execute::set_property(object, "_eventsCount", Value::Number(0.0));
    object = execute::set_property(object, "domain", Value::Undefined);
    let capture = args
        .first()
        .filter(|value| matches!(value, Value::Object(_) | Value::ObjectAlias(_)))
        .and_then(|options| {
            let value = execute::get_property_result(options, "captureRejections").ok()?;
            if !matches!(value, Value::Undefined | Value::Boolean(_)) {
                return Some(Err(crate::modules::buffer_enc::invalid_arg_type(format!(
                    "The \"options.captureRejections\" property must be of type boolean.{}",
                    crate::modules::util::invalid_arg_received(&value)
                ))));
            }
            Some(Ok(matches!(value, Value::Boolean(true))))
        })
        .transpose()?
        .unwrap_or_else(|| CAPTURE_REJECTIONS.with(Cell::get));
    if capture {
        object = execute::set_property(object, "Symbol.kCapture\0quench", Value::Boolean(true));
    }
    if let Some(id) = emitter_id(&object) {
        if let Some(emitter) = state.borrow().emitters.get(id) {
            emitter.borrow_mut().capture_rejections = capture;
        }
    }
    let object = install_emitter_props(object, false)?;
    if let Some(domain) = crate::modules::domain::current(state) {
        return crate::modules::domain::attach_member(state, &domain, object);
    }
    Ok(object)
}

/// Build a fresh EventEmitter-backed object with all standard emitter
/// methods installed. Shared by `net` for its server/socket objects.
pub fn new_emitter_object(state: &Rc<RefCell<HostState>>) -> Result<Value, VmError> {
    new_emitter(state, &[])
}

fn install_emitter_props(mut object: Value, include_constructor: bool) -> Result<Value, VmError> {
    for (key, value) in emitter_props()
        .into_iter()
        .filter(|(key, _)| include_constructor || *key != "constructor")
    {
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

/// Canonical prototype surface shared by the module export and bootstrap
/// global constructor. Keeping one declaration prevents host/JS realms from
/// drifting on deletion and override semantics.
pub fn emitter_prototype() -> Result<Value, VmError> {
    install_emitter_props(host_api::object(Vec::new()), true)
}

fn emitter_props() -> Vec<(&'static str, Value)> {
    let on = cap("events:on", 0x0102);
    let remove_listener = cap("events:removeListener", 0x0106);
    let constructor = cap("events:EventEmitter", 0x0100);
    let _ =
        execute::set_callable_property(&constructor, "name", Value::String("EventEmitter".into()));
    vec![
        ("constructor", constructor),
        ("on", on.clone()),
        ("addListener", on),
        ("once", cap("events:once", 0x0105)),
        ("emit", cap("events:emit", 0x0103)),
        ("removeListener", remove_listener.clone()),
        ("off", remove_listener),
        (
            "removeAllListeners",
            cap("events:removeAllListeners", 0x0107),
        ),
        ("listeners", cap("events:listeners", 0x0108)),
        ("rawListeners", cap("events:rawListeners", 0x0127)),
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

pub fn abort_listener_callback(
    _state: &Rc<RefCell<HostState>>,
    args: &[Value],
) -> Result<Value, VmError> {
    let listener = args.first().ok_or(VmError::NotCallable)?;
    let event = args.get(1).cloned().unwrap_or(Value::Undefined);
    execute::call(listener, &Value::Undefined, &[event])
}

pub fn abort_listener_dispose(
    state: &Rc<RefCell<HostState>>,
    args: &[Value],
) -> Result<Value, VmError> {
    let signal = args.first().ok_or_else(|| execute::type_error("signal"))?;
    let wrapped = args.get(1).ok_or_else(|| execute::type_error("listener"))?;
    crate::modules::event_target::remove_event_listener(
        state,
        Some(signal),
        &[Value::String("abort".to_string()), wrapped.clone()],
    )
}

pub fn add_abort_listener(
    state: &Rc<RefCell<HostState>>,
    args: &[Value],
) -> Result<Value, VmError> {
    let signal = args.first().cloned().unwrap_or(Value::Undefined);
    let listener = args.get(1).cloned().unwrap_or(Value::Undefined);
    if !quench_runtime::is_callable(&execute::get_property(&signal, "addEventListener")) {
        return Err(coded_abort_error("signal", "AbortSignal"));
    }
    if !quench_runtime::is_callable(&listener) {
        return Err(coded_abort_error("listener", "function"));
    }
    let wrapped = quench_runtime::host_api::bound_capability_with_arguments(
        quench_runtime::ops::HostCapabilityRef {
            realm: quench_runtime::ops::RealmId::ROOT,
            kind: quench_runtime::ops::HostCapabilityKind::Custom(
                crate::registry::SPEC_EVENTS_ABORT_LISTENER.cap,
            ),
        },
        vec![listener],
    );
    execute::set_callable_property(&wrapped, "\0quench:abort-listener", Value::Boolean(true))?;
    crate::modules::event_target::add_event_listener(
        state,
        Some(&signal),
        &[Value::String("abort".to_string()), wrapped.clone()],
    )?;
    if execute::is_truthy(&execute::get_property(&signal, "aborted")) {
        state.borrow_mut().event_loop.queue_microtask(
            wrapped.clone(),
            vec![host_api::object(vec![(
                "type".to_string(),
                Value::String("abort".to_string()),
            )])],
        );
    }
    let dispose = quench_runtime::host_api::bound_capability_with_arguments(
        quench_runtime::ops::HostCapabilityRef {
            realm: quench_runtime::ops::RealmId::ROOT,
            kind: quench_runtime::ops::HostCapabilityKind::Custom(
                crate::registry::SPEC_EVENTS_ABORT_DISPOSE.cap,
            ),
        },
        vec![signal, wrapped],
    );
    Ok(host_api::object(vec![(
        "Symbol.dispose".to_string(),
        dispose,
    )]))
}

fn coded_abort_error(argument: &str, expected: &str) -> VmError {
    VmError::Thrown(host_api::object(vec![
        ("name".to_string(), Value::String("TypeError".to_string())),
        (
            "code".to_string(),
            Value::String("ERR_INVALID_ARG_TYPE".to_string()),
        ),
        (
            "message".to_string(),
            Value::String(format!("The \"{argument}\" argument must be a {expected}")),
        ),
    ]))
}

/// `require('events')` is the `EventEmitter` constructor itself, with
/// the statics attached — mirroring Node's `module.exports =
/// EventEmitter; EventEmitter.EventEmitter = EventEmitter`.
pub fn build() -> Value {
    let value = crate::host::capability(crate::registry::SPEC_EVENTS_NEW);
    let _ = execute::set_callable_property(&value, "name", Value::String("EventEmitter".into()));
    // Host-backed constructors still expose the ordinary prototype contract;
    // consumers may delete or override methods just like native Node does.
    if let Ok(prototype) = emitter_prototype() {
        let _ = execute::set_callable_property(&value, "prototype", prototype);
    }
    let async_resource = crate::modules::async_hooks::build();
    let resource_constructor = execute::get_property(&async_resource, "AsyncResource");
    if let Ok(factory) = eval_function(
        r#"(Base, Resource) => {
          let asyncEmitterPrototype;
          const Emitter = function(...args) {
            const object = new Base(...args);
            const prototype = asyncEmitterPrototype || Emitter.prototype;
            let result = Object.setPrototypeOf(object, prototype);
            if (asyncEmitterPrototype) {
              for (const key of ['emit', 'emitDestroy', 'asyncId', 'triggerAsyncId', 'asyncResource']) {
                const descriptor = Object.getOwnPropertyDescriptor(prototype, key);
                if (descriptor) result = Object.defineProperty(result, key, descriptor);
              }
            }
            return result;
          };
          Emitter.prototype = Base.prototype;
          const owners = new WeakMap();
          const EventEmitterAsyncResource = class EventEmitterAsyncResource extends Emitter {
          constructor(nameOrOptions, options) {
            const config = nameOrOptions && typeof nameOrOptions === 'object'
              ? nameOrOptions : (options || {});
            const positionalType = typeof nameOrOptions === 'string' ? nameOrOptions : undefined;
            super(config);
            const type = positionalType || config.name || (new.target && new.target.name) || this.constructor.name || 'EventEmitterAsyncResource';
            this._asyncResource = new Resource(type, config);
            owners.set(this._asyncResource, this);
            Object.defineProperty(this._asyncResource, 'eventEmitter', {
              configurable: true,
              get: () => owners.get(this._asyncResource),
            });
            return Object.setPrototypeOf(this, EventEmitterAsyncResource.prototype);
          }
          emit(event, ...args) {
            if (!this._asyncResource) throw new TypeError('Cannot read private member');
            this._asyncResource.emitBefore();
            try { return Base.prototype.emit.call(this, event, ...args); }
            finally { this._asyncResource.emitAfter(); }
          }
          emitDestroy() {
            if (!this._asyncResource) throw new TypeError('Cannot read private member');
            this._asyncResource.emitDestroy();
            return this;
          }
          get asyncId() {
            if (!this._asyncResource) {
              const error = new TypeError('Cannot read private member');
              error.stack = 'TypeError: Cannot read private member\\n    at get asyncId';
              throw error;
            }
            return this._asyncResource.asyncId();
          }
          get triggerAsyncId() {
            if (!this._asyncResource) {
              const error = new TypeError('Cannot read private member');
              error.stack = 'TypeError: Cannot read private member\\n    at get triggerAsyncId';
              throw error;
            }
            return this._asyncResource.triggerAsyncId();
          }
          get asyncResource() {
            if (!this._asyncResource) {
              const error = new TypeError('Cannot read private member');
              error.stack = 'TypeError: Cannot read private member\\n    at get asyncResource';
              throw error;
            }
            owners.set(this._asyncResource, this);
            return this._asyncResource;
          }
          };
          asyncEmitterPrototype = EventEmitterAsyncResource.prototype;
          return EventEmitterAsyncResource;
        }"#,
    ) {
        if let Ok(async_emitter) = execute::call(
            &factory,
            &Value::Undefined,
            &[value.clone(), resource_constructor],
        ) {
            let _ =
                execute::set_callable_property(&value, "EventEmitterAsyncResource", async_emitter);
        }
    }
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
    let on = eval_function(
        r#"(emitter, event, options) => {
          if (options !== undefined && (options === null || typeof options !== "object")) {
            const error = new TypeError("The options argument must be an object");
            error.code = "ERR_INVALID_ARG_TYPE";
            throw error;
          }
          options ||= {};
          const target = typeof emitter?.on === "function";
          if (!target && typeof emitter?.addEventListener !== "function") {
            const error = new TypeError("The emitter must be an EventEmitter or EventTarget");
            error.code = "ERR_INVALID_ARG_TYPE";
            throw error;
          }
          if (options.signal !== undefined &&
              (options.signal === null || typeof options.signal !== "object" ||
               typeof options.signal.addEventListener !== "function")) {
            const error = new TypeError("The signal option must be an AbortSignal");
            error.code = "ERR_INVALID_ARG_TYPE";
            throw error;
          }
          let queue = [], waiters = Object.create(null), waiterCount = 0;
          let nextWaiter = 0, done = false, failure;
          const takeWaiter = () => {
            for (let index = 0; index < nextWaiter; index++) {
              const waiter = waiters[index];
              if (waiter) {
                delete waiters[index];
                waiterCount--;
                return waiter;
              }
            }
          };
          const finish = (error) => {
            if (done) return;
            failure = error;
            done = true;
            cleanup();
            if (error) {
              const waiter = takeWaiter();
              // Promise rejection is delivered at the microtask boundary;
              // callers commonly attach Promise.allSettled/then handlers in
              // the same turn as emitter.emit('error').
              if (waiter) queueMicrotask(() => waiter.reject(error));
            }
            while (waiterCount) {
              const waiter = takeWaiter();
              if (waiter) waiter.resolve({ value: undefined, done: true });
            }
          };
          const push = (value) => {
            if (done) return;
            const waiter = takeWaiter();
            if (waiter) waiter.resolve({ value, done: false });
            else queue.push(value);
          };
          const onEvent = (...args) => push(args);
          const onError = (error) => finish(error);
          const onAbort = () => finish(Object.assign(new Error("The operation was aborted"), { name: "AbortError", code: "ABORT_ERR" }));
          const updateSignalEvents = () => {
            if (!options.signal) return;
            let events = options.signal.__quenchAbortEvents;
            if (!events || typeof events.get !== "function") {
              events = new Map([["abort", { size: 0 }]]);
              options.signal.__quenchAbortEvents = events;
            }
            options.signal["Symbol.for.quench.event_target.events\0"] = events;
            const abort = events.get("abort");
            if (abort) abort.size = options.signal.__quenchAbortEventCount || 0;
          };
          const cleanup = () => {
            if (target) {
              emitter.removeListener?.(event, onEvent);
              emitter.removeListener?.("error", onError);
            } else emitter.removeEventListener?.(event, onEvent);
            options.signal?.removeEventListener?.("abort", onAbort);
            if (options.signal && options.signal.__quenchAbortEventCount) {
              options.signal.__quenchAbortEventCount--;
            }
            updateSignalEvents();
          };
          if (target) { emitter.on(event, onEvent); if (event !== "error") emitter.on("error", onError); }
          else emitter.addEventListener(event, onEvent);
          if (options.signal?.aborted) onAbort();
          else if (options.signal) {
            options.signal.__quenchAbortEventCount =
              (options.signal.__quenchAbortEventCount || 0) + 1;
            updateSignalEvents();
            options.signal.addEventListener?.("abort", onAbort, { once: true });
          }
          const iterator = {
            next() {
              if (queue.length) return Promise.resolve({ value: queue.shift(), done: false });
              if (failure) return Promise.reject(failure);
              if (done) return Promise.resolve({ value: undefined, done: true });
              const promise = new Promise((resolve, reject) => {
                waiters[nextWaiter++] = { resolve, reject };
                waiterCount++;
              });
              // The iterator owns the rejection edge; consumers may attach
              // Promise combinators later in the same turn. Mark this source
              // promise handled without changing the value returned to them.
              promise.catch(() => {});
              return promise;
            },
            return() { finish(); return Promise.resolve({ value: undefined, done: true }); },
            throw(reason) {
              if (reason === undefined) {
                const error = new TypeError("The \"EventEmitter.AsyncIterator\" property must be an instance of Error. Received undefined");
                throw error;
              }
              finish(reason);
              return Promise.reject(reason);
            },
            [Symbol.asyncIterator]() { return this; }
          };
          return iterator;
        }"#,
    )
    .unwrap_or(Value::Undefined);
    let add_abort_listener = crate::host::capability(crate::registry::SPEC_EVENTS_ADD_ABORT);
    let props: Vec<(String, Value)> = vec![
        ("EventEmitter".to_string(), value.clone()),
        (
            "errorMonitor".to_string(),
            Value::String("Symbol.for.events.errorMonitor\0".into()),
        ),
        (
            "captureRejectionSymbol".to_string(),
            Value::String("Symbol.for.nodejs.rejection\0".into()),
        ),
        ("once".to_string(), once),
        ("on".to_string(), on),
        ("addAbortListener".to_string(), add_abort_listener),
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
    let descriptor = host_api::object(vec![
        ("get".into(), cap("events:captureRejections:get", 0x0104)),
        ("set".into(), cap("events:captureRejections:set", 0x0119)),
        ("enumerable".into(), Value::Boolean(false)),
        ("configurable".into(), Value::Boolean(true)),
    ]);
    let _ = execute::define_property(value.clone(), "captureRejections", descriptor);
    let descriptor = host_api::object(vec![
        ("get".into(), cap("events:defaultMaxListeners:get", 0x0125)),
        ("set".into(), cap("events:defaultMaxListeners:set", 0x0126)),
        ("enumerable".into(), Value::Boolean(false)),
        ("configurable".into(), Value::Boolean(true)),
    ]);
    let _ = execute::define_property(value.clone(), "defaultMaxListeners", descriptor);
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
