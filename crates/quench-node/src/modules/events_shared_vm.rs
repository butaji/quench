//! Shared-VM projection of the existing Node EventEmitter bridge.

use crate::host::NodeHost;
use quench_runtime::{NativeContext, RootId, RootedError};

const EVENTS_API: &str = quench_js_check::checked_js!(
    r#"(() => {
  const invalid = (message) => {
    const error = new TypeError(message);
    error.code = "ERR_INVALID_ARG_TYPE";
    return error;
  };

  const once = (emitter, event, options) => {
    if (options !== undefined && (options === null || typeof options !== "object")) {
      return Promise.reject(invalid("The options argument must be an object"));
    }
    options ||= {};
    const signal = options.signal;
    if (signal !== undefined &&
        (signal === null || typeof signal !== "object" ||
         typeof signal.addEventListener !== "function")) {
      return Promise.reject(invalid("The signal option must be an AbortSignal"));
    }
    const isEmitter = typeof emitter?.once === "function";
    if (!isEmitter && typeof emitter?.addEventListener !== "function") {
      return Promise.reject(invalid("The emitter must be an EventEmitter or EventTarget"));
    }
    return new Promise((resolve, reject) => {
      const remove = () => {
        if (isEmitter) {
          emitter.removeListener?.(event, onEvent);
          if (event !== "error") emitter.removeListener?.("error", onError);
        } else {
          emitter.removeEventListener?.(event, onEvent);
        }
        signal?.removeEventListener?.("abort", onAbort);
      };
      const onEvent = (...args) => { remove(); resolve(args); };
      const onError = (error) => { remove(); reject(error); };
      const onAbort = () => {
        remove();
        const error = new Error("The operation was aborted");
        error.name = "AbortError";
        error.code = "ABORT_ERR";
        reject(error);
      };
      if (isEmitter) {
        emitter.once(event, onEvent);
        if (event !== "error") emitter.once("error", onError);
      } else {
        emitter.addEventListener(event, onEvent, { once: true });
      }
      if (signal?.aborted) onAbort();
      else signal?.addEventListener?.("abort", onAbort, { once: true });
      queueMicrotask(() => { if (signal?.aborted) onAbort(); });
    });
  };

  return { once };
})()"#
);

pub(crate) fn module(context: &mut NativeContext<'_, NodeHost>) -> Result<RootId, RootedError> {
    let global = context.global_root()?;
    let class_key = context.string_rooted("__nodeEventEmitter");
    let module = context.get_property_rooted(global, class_key)?;
    set(context, module, "EventEmitter", module)?;
    super::emitter_async_resource_shared_vm::install(context, module)?;

    let inspect = crate::modules::util_shared_vm::internal_inspect(context)?;
    let internal_inspect = context.evaluate_script_rooted(
        "Symbol.for('quench.internal.eventEmitter.inspect')",
        "node:events/internal-inspect-symbol",
    )?;
    if !context.set_property_rooted(module, internal_inspect, inspect, module)? {
        return Err(RootedError::host(
            "cannot install the EventEmitter error inspector",
        ));
    }

    let api = context.evaluate_script_rooted(EVENTS_API, "node:events/shared-api.js")?;
    let once = get(context, api, "once")?;
    set(context, module, "once", once)?;
    let state = context.host_mut().shared_state();
    let data = state
        .borrow()
        .event_targets_api
        .ok_or_else(|| RootedError::host("EventTarget API missing"))?;
    let native = context.host_function_with_data(
        crate::host::shared_vm::operation("eventsListenerCount"),
        data,
    )?;
    let wrapper = context.evaluate_script_rooted(
        "(native) => function listenerCount(emitter, event) { return native(emitter, event); }",
        "node:events/listener-count.js",
    )?;
    let undefined = context.undefined();
    let count = context.call_rooted(wrapper, undefined, &[native])?;
    set(context, module, "listenerCount", count)?;
    let native = context.host_function_with_data(
        crate::host::shared_vm::operation("eventsGetEventListeners"),
        data,
    )?;
    let wrapper = context.evaluate_script_rooted(
        "(native) => function getEventListeners(emitter, type) { return native(emitter, type); }",
        "node:events/get-event-listeners.js",
    )?;
    let listeners = context.call_rooted(wrapper, undefined, &[native])?;
    set(context, module, "getEventListeners", listeners)?;
    let native = context.host_function_with_data(
        crate::host::shared_vm::operation("eventsAddAbortListener"),
        data,
    )?;
    let wrapper = context.evaluate_script_rooted(
        "(native) => function addAbortListener(signal, listener) { return native(signal, listener); }",
        "node:events/add-abort-listener.js",
    )?;
    let add = context.call_rooted(wrapper, undefined, &[native])?;
    set(context, module, "addAbortListener", add)?;
    let symbol = context.evaluate_script_rooted(
        "Symbol.for('events.errorMonitor')",
        "node:events/errorMonitor",
    )?;
    set(context, module, "errorMonitor", symbol)?;
    let capture_symbol = context.evaluate_script_rooted(
        "Symbol.for('nodejs.rejection')",
        "node:events/captureRejectionSymbol",
    )?;
    set(context, module, "captureRejectionSymbol", capture_symbol)?;
    Ok(module)
}

fn set(
    context: &mut NativeContext<'_, NodeHost>,
    object: RootId,
    name: &str,
    value: RootId,
) -> Result<(), RootedError> {
    let key = context.string_rooted(name);
    if context.set_property_rooted(object, key, value, object)? {
        Ok(())
    } else {
        Err(RootedError::host(format!(
            "cannot install shared events property {name}"
        )))
    }
}

fn get(
    context: &mut NativeContext<'_, NodeHost>,
    object: RootId,
    name: &str,
) -> Result<RootId, RootedError> {
    let key = context.string_rooted(name);
    context.get_property_rooted(object, key)
}

pub(crate) fn initialize_targets(
    context: &mut NativeContext<'_, NodeHost>,
) -> Result<(), RootedError> {
    let global = context.global_root()?;
    let symbol = get(context, global, "Symbol")?;
    let undefined = context.undefined();
    let description = context.string_rooted("kEvents");
    let records_key = context.call_rooted(symbol, undefined, &[description])?;
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
    ] {
        let key = context.symbol_rooted(Some(description));
        set(context, data, field, key)?;
    }
    let description = context.string_rooted("nodejs.rejection");
    let rejection_key = context.call_rooted(registry, symbol, &[description])?;
    set(context, data, "rejectionKey", rejection_key)?;
    let description = context.string_rooted("events.errorMonitor");
    let monitor_key = context.call_rooted(registry, symbol, &[description])?;
    set(context, data, "monitorKey", monitor_key)?;
    let maximum = context.number(super::event_limits_shared_vm::DEFAULT_MAX_LISTENERS);
    set(context, data, "defaultMaxListeners", maximum)?;
    set(context, data, "maxKey", max_key)?;
    set(context, data, "warnedKey", warned_key)?;
    let assign = context.evaluate_script_rooted(
        "(object, key, value) => { 'use strict'; object[key] = value; }",
        "node:events/assignment.js",
    )?;
    set(context, data, "assign", assign)?;
    let delete = context.evaluate_script_rooted(
        "(object, key) => { 'use strict'; delete object[key]; }",
        "node:events/deletion.js",
    )?;
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
    let dispose_factory = context.evaluate_script_rooted(
        "(dispose, key) => ({ __proto__: null, [key]() { dispose(); } })",
        "node:events/abort-listener-disposable.js",
    )?;
    set(context, data, "disposeFactory", dispose_factory)?;
    set(context, data, "recordsKey", records_key)?;
    set(context, data, "brandKey", brand_key)?;
    let retained = context.retain(data)?;
    context
        .host_mut()
        .shared_state()
        .borrow_mut()
        .event_targets_api = Some(retained);
    // The DOM event family is built on first use; the natives that read `abortState` reach
    // it through the same group.
    let initialize = context.host_function_with_data(
        crate::host::shared_vm::operation("eventGlobalsInitialize"),
        data,
    )?;
    context.define_lazy_data_properties_rooted(global, EVENT_GLOBALS, false, initialize)?;
    let abort_state = context.host_function(crate::host::shared_vm::operation(
        "eventAbortStateInitialize",
    ))?;
    context.define_lazy_data_properties_rooted(data, &["abortState"], true, abort_state)?;
    Ok(())
}

/// The globals the abort and event-target polyfills define, in definition order.
const EVENT_GLOBALS: &[&str] = &[
    "DOMException",
    "AbortSignal",
    "AbortController",
    "Event",
    "EventTarget",
    "MessageEvent",
    "MessagePort",
    "MessageChannel",
];

/// Builds the DOM event family: the abort polyfill, `EventTarget` and its relatives.
pub(crate) fn initialize_event_globals(
    context: &mut NativeContext<'_, NodeHost>,
    _: RootId,
    _: &[RootId],
) -> Result<RootId, RootedError> {
    let data = context.host_function_data()?;
    let global = context.global_root()?;
    let undefined = context.undefined();
    let records_key = get(context, data, "recordsKey")?;
    let brand_key = get(context, data, "brandKey")?;
    let max_key = get(context, data, "maxKey")?;
    let warned_key = get(context, data, "warnedKey")?;
    let max_default = context.host_function_with_data(
        crate::host::shared_vm::operation("eventsDefaultMaxGet"),
        data,
    )?;
    let abort_add = context.host_function_with_data(
        crate::host::shared_vm::operation("abortAddEventListener"),
        data,
    )?;
    let abort_dispatch = context.host_function_with_data(
        crate::host::shared_vm::operation("abortDispatchListeners"),
        data,
    )?;
    let abort_get_handler = context
        .host_function_with_data(crate::host::shared_vm::operation("abortGetHandler"), data)?;
    let abort_set_handler = context
        .host_function_with_data(crate::host::shared_vm::operation("abortSetHandler"), data)?;
    let abort_remove = context.host_function_with_data(
        crate::host::shared_vm::operation("abortRemoveEventListener"),
        data,
    )?;
    let handler_factory = context.evaluate_script_rooted(
        "(invoke) => function eventHandler(...args) { return invoke(this, args); }",
        "node:events/abort-handler.js",
    )?;
    set(context, data, "abortHandlerFactory", handler_factory)?;
    let abort_factory = context.evaluate_script_rooted(
        crate::polyfills::shared_vm::ABORT,
        "node:bootstrap/shared-vm/abort.js",
    )?;
    let abort_max = context.number(super::event_limits_shared_vm::ABORT_SIGNAL_MAX_LISTENERS);
    let abort_state = context.call_rooted(
        abort_factory,
        undefined,
        &[
            max_key,
            warned_key,
            abort_max,
            abort_add,
            abort_dispatch,
            abort_get_handler,
            abort_set_handler,
            abort_remove,
        ],
    )?;
    #[cfg(feature = "profile-memory")]
    context.profile_memory_checkpoint("node_abort_polyfill");
    let factory = context.evaluate_script_rooted(
        crate::polyfills::shared_vm::EVENT_TARGET,
        "node:bootstrap/shared-vm/event-target.js",
    )?;
    context.call_rooted(
        factory,
        undefined,
        &[records_key, max_key, warned_key, max_default],
    )?;
    let event = get(context, global, "Event")?;
    // Node exposes immutable DOM event phase constants on the constructor.
    for (name, phase) in [
        ("NONE", 0.0),
        ("CAPTURING_PHASE", 1.0),
        ("AT_TARGET", 2.0),
        ("BUBBLING_PHASE", 3.0),
    ] {
        let key = context.string_rooted(name);
        let value = context.number(phase);
        context.define_data_property_rooted(event, key, value, false, true, false)?;
    }
    let target = get(context, global, "EventTarget")?;
    let yes = context.boolean(true);
    context.define_data_property_rooted(target, brand_key, yes, true, true, true)?;
    let signal = get(context, global, "AbortSignal")?;
    context.set_prototype_rooted(signal, target)?;
    let signal_prototype = get(context, signal, "prototype")?;
    let target_prototype = get(context, target, "prototype")?;
    context.set_prototype_rooted(signal_prototype, target_prototype)?;
    set(context, data, "abortState", abort_state)?;
    Ok(undefined)
}

/// `abortState` exists once the event family does.
pub(crate) fn initialize_abort_state(
    context: &mut NativeContext<'_, NodeHost>,
    _: RootId,
    _: &[RootId],
) -> Result<RootId, RootedError> {
    let global = context.global_root()?;
    get(context, global, "AbortSignal")?;
    Ok(context.undefined())
}

pub(crate) fn listener_count(
    context: &mut NativeContext<'_, NodeHost>,
    _: RootId,
    args: &[RootId],
) -> Result<RootId, RootedError> {
    let undefined = context.undefined();
    let emitter = args.first().copied().unwrap_or(undefined);
    let event = args.get(1).copied().unwrap_or(undefined);
    let method = get(context, emitter, "listenerCount")?;
    if context.is_callable_rooted(method)? {
        let method = get(context, emitter, "listenerCount")?;
        return context.call_rooted(method, emitter, &[event]);
    }
    let data = context.host_function_data()?;
    validate_event_target(context, data, emitter)?;
    let abort_state = get(context, data, "abortState")?;
    let state = context.call_rooted(abort_state, undefined, &[emitter])?;
    if context.truthy_rooted(state)? {
        if context.string_text(event)?.as_deref() != Some("abort") {
            return Ok(context.number(0.0));
        }
        return get(context, state, "listenerCount");
    }
    // Node's Map lookup preserves key type rather than coercing property keys.
    if context.string_text(event)?.is_none() && !context.is_symbol_rooted(event)? {
        return Ok(context.number(0.0));
    }
    let key = get(context, data, "recordsKey")?;
    let records = context.get_property_rooted(emitter, key)?;
    if !context.truthy_rooted(records)? {
        return Ok(context.number(0.0));
    }
    let listeners = context.get_property_rooted(records, event)?;
    if !context.truthy_rooted(listeners)? {
        return Ok(context.number(0.0));
    }
    get(context, listeners, "length")
}

fn validate_event_target(
    context: &mut NativeContext<'_, NodeHost>,
    data: RootId,
    emitter: RootId,
) -> Result<(), RootedError> {
    let constructor = get(context, emitter, "constructor")?;
    let constructor_value = context
        .rooted_value(constructor)
        .ok_or_else(|| RootedError::host("constructor root missing"))?;
    let branded = if constructor_value.is_null() || constructor_value.is_undefined() {
        false
    } else {
        let key = get(context, data, "brandKey")?;
        let brand = context.get_property_rooted(constructor, key)?;
        context.truthy_rooted(brand)?
    };
    if !branded {
        return Err(super::util_shared_vm::argument_type_error(
            context,
            emitter,
            "emitter",
            "EventEmitter or EventTarget",
        )?);
    }
    Ok(())
}

/// Return a fresh projection of the canonical listener records.
pub(crate) fn get_event_listeners(
    context: &mut NativeContext<'_, NodeHost>,
    _: RootId,
    args: &[RootId],
) -> Result<RootId, RootedError> {
    let undefined = context.undefined();
    let emitter = args.first().copied().unwrap_or(undefined);
    let event = args.get(1).copied().unwrap_or(undefined);
    let method = get(context, emitter, "listeners")?;
    if context.is_callable_rooted(method)? {
        let method = get(context, emitter, "listeners")?;
        return context.call_rooted(method, emitter, &[event]);
    }
    let data = context.host_function_data()?;
    validate_event_target(context, data, emitter)?;
    let abort_state = get(context, data, "abortState")?;
    let state = context.call_rooted(abort_state, undefined, &[emitter])?;
    let records = if context.truthy_rooted(state)? {
        if context.string_text(event)?.as_deref() != Some("abort") {
            return context.array_rooted(&[]);
        }
        get(context, state, "listeners")?
    } else {
        if context.string_text(event)?.is_none() && !context.is_symbol_rooted(event)? {
            return context.array_rooted(&[]);
        }
        let key = get(context, data, "recordsKey")?;
        let records = context.get_property_rooted(emitter, key)?;
        if !context.truthy_rooted(records)? {
            return context.array_rooted(&[]);
        }
        context.get_property_rooted(records, event)?
    };
    let mut listeners = Vec::new();
    if context.truthy_rooted(records)? {
        for entry in listener_records(context, records)? {
            listeners.push(get(context, entry, "listener")?);
        }
    }
    context.array_rooted(&listeners)
}

pub(super) fn listener_records(
    context: &mut NativeContext<'_, NodeHost>,
    records: RootId,
) -> Result<Vec<RootId>, RootedError> {
    let length = get(context, records, "length")?;
    let length = context
        .rooted_value(length)
        .and_then(|value| value.as_number())
        .ok_or_else(|| RootedError::host("EventTarget listener length missing"))?
        as usize;
    (0..length)
        .map(|index| get(context, records, &index.to_string()))
        .collect()
}
