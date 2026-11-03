//! The `events` module: the shared EventEmitter class with Node's static helpers, and the
//! event group's shared data (`SharedNodeState::event_targets_api`) that builds the DOM event
//! family on first use.

use crate::host::NodeHost;
use quench_runtime::{NativeContext, RootId, RootedError};

type Context<'a> = NativeContext<'a, NodeHost>;

/// The `events` module's static functions: (property, function name, operation, length).
/// Node assigns `listenerCount` an anonymous function.
const STATICS: [(&str, &str, &str, f64); 5] = [
    ("once", "once", "eventsOnce", 2.0),
    ("on", "on", "eventsOn", 2.0),
    ("getEventListeners", "getEventListeners", "eventsGetEventListeners", 2.0),
    ("addAbortListener", "addAbortListener", "eventsAddAbortListener", 2.0),
    ("listenerCount", "", "eventsListenerCount", 2.0),
];

pub(crate) fn module(context: &mut Context<'_>) -> Result<RootId, RootedError> {
    let global = context.global_root()?;
    let module = get(context, global, "__nodeEventEmitter")?;
    set(context, module, "EventEmitter", module)?;
    for (property, name, operation, length) in STATICS {
        let function = context.host_function_named(crate::host::shared_vm::operation(operation), name)?;
        let length = context.number(length);
        context.define_named_data_property_rooted(function, "length", length, false, false, true)?;
        set(context, module, property, function)?;
    }
    let data = super::event_target_shared_vm::api(context)?;
    super::emitter_async_resource_shared_vm::install(context, module)?;
    let monitor = get(context, data, "monitorKey")?;
    set(context, module, "errorMonitor", monitor)?;
    Ok(module)
}

fn set(context: &mut Context<'_>, object: RootId, name: &str, value: RootId) -> Result<(), RootedError> {
    let key = context.string_rooted(name);
    if context.set_property_rooted(object, key, value, object)? {
        Ok(())
    } else {
        Err(RootedError::host(format!("cannot install shared events property {name}")))
    }
}

fn get(context: &mut Context<'_>, object: RootId, name: &str) -> Result<RootId, RootedError> {
    context.get_named_property_rooted(object, name)
}

pub(crate) fn initialize_targets(context: &mut Context<'_>) -> Result<(), RootedError> {
    let global = context.global_root()?;
    let symbol = get(context, global, "Symbol")?;
    let undefined = context.undefined();
    let registry = get(context, symbol, "for")?;
    let description = context.string_rooted("nodejs.event_target");
    let brand_key = context.call_rooted(registry, symbol, &[description])?;
    let description = context.string_rooted("events.maxEventTargetListeners");
    let max_key = context.call_rooted(symbol, undefined, &[description])?;
    let description = context.string_rooted("events.maxEventTargetListenersWarned");
    let warned_key = context.call_rooted(symbol, undefined, &[description])?;
    let data = context.null_object_rooted()?;
    for (field, description) in [
        ("captureKey", "kCapture"),
        ("shapeKey", "shapeMode"),
        ("emittingKey", "events.emitting"),
        ("monitorKey", "events.errorMonitor"),
    ] {
        let key = context.symbol_rooted(Some(description));
        set(context, data, field, key)?;
    }
    for field in [
        super::event_target_shared_vm::EVENT_SLOT,
        super::event_target_shared_vm::TARGET_SLOT,
        super::event_target_shared_vm::DETAIL_SLOT,
        super::event_target_shared_vm::LISTENER_HOOK_SLOT,
        super::abort_controller_shared_vm::SIGNAL_SLOT,
        super::abort_controller_shared_vm::CONTROLLER_SLOT,
        super::dom_exception_shared_vm::SLOT,
        super::emitter_async_resource_shared_vm::RESOURCE_SLOT,
        super::emitter_async_resource_shared_vm::EMITTER_SLOT,
    ] {
        let key = context.private_symbol_rooted(Some(field));
        set(context, data, field, key)?;
    }
    let description = context.string_rooted("nodejs.rejection");
    let rejection_key = context.call_rooted(registry, symbol, &[description])?;
    set(context, data, "rejectionKey", rejection_key)?;
    let maximum = context.number(super::event_limits_shared_vm::DEFAULT_MAX_LISTENERS);
    set(context, data, "defaultMaxListeners", maximum)?;
    set(context, data, "maxKey", max_key)?;
    set(context, data, "warnedKey", warned_key)?;
    let assign = context.host_function_named(crate::host::shared_vm::operation("strictAssign"), "assign")?;
    set(context, data, "assign", assign)?;
    let delete = context.host_function_named(crate::host::shared_vm::operation("strictDelete"), "delete")?;
    set(context, data, "delete", delete)?;
    let string = get(context, global, "String")?;
    set(context, data, "stringCtor", string)?;
    let description = context.string_rooted("kResistStopPropagation");
    let resist_key = context.call_rooted(symbol, undefined, &[description])?;
    set(context, data, "resistKey", resist_key)?;
    let options = context.null_object_rooted()?;
    let yes = context.boolean(true);
    set(context, options, "once", yes)?;
    context.define_data_property_rooted(options, resist_key, yes, true, true, true)?;
    let object = get(context, global, "Object")?;
    let freeze = get(context, object, "freeze")?;
    context.call_rooted(freeze, object, &[options])?;
    set(context, data, "abortListenerOptions", options)?;
    let dispose_symbol = get(context, symbol, "dispose")?;
    set(context, data, "disposeKey", dispose_symbol)?;
    let dispose_factory = context.host_function(crate::host::shared_vm::operation("disposableFactory"))?;
    set(context, data, "disposeFactory", dispose_factory)?;
    set(context, data, "brandKey", brand_key)?;
    let retained = context.retain(data)?;
    context.host_mut().shared_state().borrow_mut().event_targets_api = Some(retained);
    // The DOM event family is built on first use.
    let initialize = context.host_function(crate::host::shared_vm::operation("eventGlobalsInitialize"))?;
    context.define_lazy_data_properties_rooted(global, EVENT_GLOBALS, false, initialize)?;
    let messaging = context.host_function(crate::host::shared_vm::operation("messagingGlobalsInitialize"))?;
    context.define_lazy_data_properties_rooted(global, MESSAGING_GLOBALS, false, messaging)?;
    Ok(())
}

/// The globals the messaging installer defines, in definition order.
pub(crate) const MESSAGING_GLOBALS: &[&str] = &["MessagePort", "MessageChannel", "BroadcastChannel"];
/// The DOM event family's globals, in definition order, with the data member holding each.
const EVENT_CLASSES: [(&str, &str); 6] = [
    ("DOMException", "domExceptionCtor"),
    ("AbortSignal", "abortSignalCtor"),
    ("AbortController", "abortControllerCtor"),
    ("Event", "eventCtor"),
    ("EventTarget", "eventTargetCtor"),
    ("CustomEvent", "customEventCtor"),
];
pub(crate) const EVENT_GLOBALS: &[&str] = &{
    let mut names = [""; EVENT_CLASSES.len()];
    let mut index = 0;
    while index < names.len() {
        names[index] = EVENT_CLASSES[index].0;
        index += 1;
    }
    names
};

/// The DOM event family, built once into the event group's data.
pub(crate) fn event_family(context: &mut Context<'_>) -> Result<RootId, RootedError> {
    let data = super::event_target_shared_vm::api(context)?;
    let built = get(context, data, "eventTargetCtor")?;
    if !context.is_callable_rooted(built)? {
        super::dom_exception_shared_vm::class(context)?;
        super::event_target_shared_vm::install(context, data)?;
        super::abort_controller_shared_vm::install(context, data)?;
    }
    Ok(data)
}

/// The lazy globals' initializer: defines the DOM event family's classes.
pub(crate) fn initialize_event_globals(context: &mut Context<'_>, _: RootId, _: &[RootId]) -> Result<RootId, RootedError> {
    let data = event_family(context)?;
    let global = context.global_root()?;
    for (name, member) in EVENT_CLASSES {
        let class = get(context, data, member)?;
        let key = context.string_rooted(name);
        context.define_data_property_rooted(global, key, class, true, false, true)?;
    }
    Ok(context.undefined())
}

/// `events.listenerCount(emitter, type)`: the emitter's own `listenerCount`, else
/// `EventEmitter.prototype.listenerCount` on it.
pub(crate) fn listener_count(context: &mut Context<'_>, _: RootId, args: &[RootId]) -> Result<RootId, RootedError> {
    let emitter = argument(context, args, 0);
    let event = argument(context, args, 1);
    let method = get(context, emitter, "listenerCount")?;
    if context.is_callable_rooted(method)? {
        return context.call_rooted(method, emitter, &[event]);
    }
    let global = context.global_root()?;
    let class = get(context, global, "__nodeEventEmitter")?;
    let prototype = get(context, class, "prototype")?;
    let count = get(context, prototype, "listenerCount")?;
    context.call_rooted(count, emitter, &[event])
}

fn invalid_emitter(context: &mut Context<'_>, emitter: RootId, expected: &str) -> RootedError {
    match super::util_shared_vm::argument_type_error(context, emitter, "emitter", expected) {
        Ok(error) | Err(error) => error,
    }
}

/// `getEventListeners(emitterOrTarget, type)`: a copy of the listeners in order.
pub(crate) fn get_event_listeners(context: &mut Context<'_>, _: RootId, args: &[RootId]) -> Result<RootId, RootedError> {
    let emitter = argument(context, args, 0);
    let event = argument(context, args, 1);
    let method = if super::event_target_shared_vm::is_object_like(context, emitter)? { get(context, emitter, "listeners")? } else { context.undefined() };
    if context.is_callable_rooted(method)? {
        return context.call_rooted(method, emitter, &[event]);
    }
    event_family(context)?;
    if !super::event_target_shared_vm::is_event_target(context, emitter)? {
        return Err(invalid_emitter(context, emitter, "EventEmitter or EventTarget"));
    }
    // Node keys its listener map by the type as given.
    if context.string_text(event)?.is_none() {
        return context.array_rooted(&[]);
    }
    let listeners = super::event_target_shared_vm::listeners(context, emitter, event)?;
    context.array_rooted(&listeners)
}

/// `addAbortListener(signal, listener)`: `listener` runs once on abort, or in a microtask
/// when the signal already aborted; the result's `[Symbol.dispose]()` unregisters it.
pub(crate) fn add_abort_listener(context: &mut Context<'_>, _: RootId, args: &[RootId]) -> Result<RootId, RootedError> {
    let signal = argument(context, args, 0);
    let listener = argument(context, args, 1);
    if context.rooted_value(signal).is_some_and(|value| value.is_undefined()) {
        return Err(super::util_shared_vm::argument_type_error(context, signal, "signal", "AbortSignal")?);
    }
    super::util_shared_vm::validate_abort_signal(context, signal, "signal")?;
    if !context.is_callable_rooted(listener)? {
        return Err(super::util_shared_vm::argument_type_error(context, listener, "listener", "function")?);
    }
    let registration = if super::abort_controller_shared_vm::aborted(context, signal)? {
        let job = context.host_function_with_data(crate::host::shared_vm::operation("abortedListenerJob"), listener)?;
        context.queue_microtask_rooted(job, &[])?;
        context.array_rooted(&[])?
    } else {
        let data = super::event_target_shared_vm::api(context)?;
        let options = get(context, data, "abortListenerOptions")?;
        let add = get(context, signal, "addEventListener")?;
        let event = context.string_rooted("abort");
        context.call_rooted(add, signal, &[event, listener, options])?;
        context.array_rooted(&[signal, listener])?
    };
    let dispose = context.host_function_with_data(crate::host::shared_vm::operation("abortListenerDispose"), registration)?;
    let data = super::event_target_shared_vm::api(context)?;
    let key = get(context, data, "disposeKey")?;
    let undefined = context.undefined();
    disposable_factory(context, undefined, &[dispose, key])
}

/// The microtask that calls a listener added to an already aborted signal.
pub(crate) fn aborted_listener_job(context: &mut Context<'_>, _: RootId, _: &[RootId]) -> Result<RootId, RootedError> {
    let listener = context.host_function_data()?;
    let undefined = context.undefined();
    context.call_rooted(listener, undefined, &[])?;
    Ok(undefined)
}

/// The disposable's operation: removes a live registration (`[signal, listener]`).
pub(crate) fn abort_listener_dispose(context: &mut Context<'_>, _: RootId, _: &[RootId]) -> Result<RootId, RootedError> {
    let registration = context.host_function_data()?;
    let parts = context.array_like_values_rooted(registration)?;
    if let [signal, listener] = parts[..] {
        let remove = get(context, signal, "removeEventListener")?;
        let event = context.string_rooted("abort");
        context.call_rooted(remove, signal, &[event, listener])?;
    }
    Ok(context.undefined())
}

fn argument(context: &mut Context<'_>, args: &[RootId], index: usize) -> RootId {
    match args.get(index) {
        Some(value) => *value,
        None => context.undefined(),
    }
}

/// `(object, key, value) => { 'use strict'; object[key] = value; }`: an assignment that
/// throws where `[[Set]]` refuses it.
pub(crate) fn strict_assign(context: &mut Context<'_>, _: RootId, args: &[RootId]) -> Result<RootId, RootedError> {
    let (object, key, value) = (argument(context, args, 0), argument(context, args, 1), argument(context, args, 2));
    if !context.set_property_rooted(object, key, value, object)? {
        let name = context.to_string(key)?;
        let error = context.type_error_rooted(&format!("Cannot assign to read only property '{name}' of object"))?;
        return Err(context.throw(error));
    }
    Ok(context.undefined())
}

/// `(object, key) => { 'use strict'; delete object[key]; }`: a deletion that throws where
/// `[[Delete]]` refuses it.
pub(crate) fn strict_delete(context: &mut Context<'_>, _: RootId, args: &[RootId]) -> Result<RootId, RootedError> {
    let (object, key) = (argument(context, args, 0), argument(context, args, 1));
    if !context.delete_property_rooted(object, key)? {
        let name = context.to_string(key)?;
        let error = context.type_error_rooted(&format!("Cannot delete property '{name}' of object"))?;
        return Err(context.throw(error));
    }
    Ok(context.undefined())
}

/// `(dispose, key) => ({ __proto__: null, [key]() { dispose(); } })`: the disposable an
/// abort listener registration returns.
pub(crate) fn disposable_factory(context: &mut Context<'_>, _: RootId, args: &[RootId]) -> Result<RootId, RootedError> {
    let (dispose, key) = (argument(context, args, 0), argument(context, args, 1));
    let disposable = context.null_object_rooted()?;
    let method = context.host_function_named_with_data(crate::host::shared_vm::operation("disposableDispose"), "[Symbol.dispose]", dispose)?;
    context.define_data_property_rooted(disposable, key, method, true, true, true)?;
    Ok(disposable)
}

/// The disposable's method: calls its `dispose`.
pub(crate) fn disposable_dispose(context: &mut Context<'_>, _: RootId, _: &[RootId]) -> Result<RootId, RootedError> {
    let dispose = context.host_function_data()?;
    let undefined = context.undefined();
    context.call_rooted(dispose, undefined, &[])?;
    Ok(context.undefined())
}
