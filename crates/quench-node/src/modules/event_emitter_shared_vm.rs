//! EventEmitter inspection derives copies and counts from its listener store.

use crate::host::NodeHost;
use quench_runtime::{NativeContext, RootId, RootedError};

const FACTORY: &str = quench_js_check::checked_js!(
    r#"(namesNative, listenersNative, rawNative, countNative, addNative, onceNative, removeNative, allNative, emitNative) => {
  'use strict';
  return {
    eventNames: function eventNames() { return namesNative(this); },
    listeners: function listeners(type) { return listenersNative(this, type); },
    rawListeners: function rawListeners(type) { return rawNative(this, type); },
    listenerCount: function listenerCount(type, listener) { return countNative(this, type, listener); },
    addListener: function addListener(type, listener) { return addNative(this, type, listener, false); },
    prependListener: function prependListener(type, listener) { return addNative(this, type, listener, true); },
    once: function once(type, listener) { return onceNative(this, type, listener, false); },
    prependOnceListener: function prependOnceListener(type, listener) { return onceNative(this, type, listener, true); },
    removeListener: function removeListener(type, listener) { return removeNative(this, type, listener); },
    removeAllListeners: function removeAllListeners(type) { return allNative(this, type, arguments.length > 0); },
    emit: function emit(type, ...args) { return emitNative(this, type, args); }
  };
}"#
);

fn get(
    context: &mut NativeContext<'_, NodeHost>,
    object: RootId,
    name: &str,
) -> Result<RootId, RootedError> {
    context.get_named_property_rooted(object, name)
}
fn undefined(context: &NativeContext<'_, NodeHost>, value: RootId) -> bool {
    context
        .rooted_value(value)
        .is_some_and(|value| value.is_undefined())
}
fn arguments(context: &mut NativeContext<'_, NodeHost>, args: &[RootId]) -> (RootId, RootId) {
    let undefined = context.undefined();
    (
        args.first().copied().unwrap_or(undefined),
        args.get(1).copied().unwrap_or(undefined),
    )
}
fn record(
    context: &mut NativeContext<'_, NodeHost>,
    emitter: RootId,
    event: RootId,
) -> Result<RootId, RootedError> {
    let events = get(context, emitter, "_events")?;
    if undefined(context, events) {
        return Ok(events);
    }
    context.get_property_rooted(events, event)
}
fn entries(
    context: &mut NativeContext<'_, NodeHost>,
    record: RootId,
) -> Result<Vec<RootId>, RootedError> {
    if undefined(context, record) {
        return Ok(Vec::new());
    }
    if context.is_callable_rooted(record)? {
        return Ok(vec![record]);
    }
    let length = get(context, record, "length")?;
    let length = context
        .rooted_value(length)
        .and_then(|value| value.as_number())
        .unwrap_or_default()
        .max(0.0) as usize;
    (0..length)
        .map(|index| {
            let key = context.string_rooted(&index.to_string());
            context.get_property_rooted(record, key)
        })
        .collect()
}

pub(crate) fn initialize(context: &mut NativeContext<'_, NodeHost>) -> Result<(), RootedError> {
    let global = context.global_root()?;
    let emitter = get(context, global, "__nodeEventEmitter")?;
    let prototype = get(context, emitter, "prototype")?;
    let data = context
        .host_mut()
        .shared_state()
        .borrow()
        .event_targets_api
        .ok_or_else(|| RootedError::host("emitter configuration missing"))?;
    let once_factory = context.evaluate_script_rooted(
        "(invoke) => function wrapper(...args) { return invoke(...args); }",
        "node:events/once-wrapper.js",
    )?;
    context.assign_named_property_rooted(data,
        "onceFactory",
        once_factory,
    )?;
    let emit_bridge = get(context, prototype, "emit")?;
    context.assign_named_property_rooted(data, "emitBridge", emit_bridge)?;
    let mut operations = [
        "emitterEventNames",
        "emitterListeners",
        "emitterRawListeners",
        "emitterListenerCount",
    ]
    .into_iter()
    .map(|name| context.host_function(crate::host::shared_vm::operation(name)))
    .collect::<Result<Vec<_>, _>>()?;
    for operation in [
        "emitterAddListener",
        "emitterOnce",
        "emitterRemoveListener",
        "emitterRemoveAllListeners",
        "emitterEmit",
    ] {
        operations.push(
            context.host_function_with_data(crate::host::shared_vm::operation(operation), data)?,
        );
    }
    let factory = context.evaluate_script_rooted(FACTORY, "node:events/inspection.js")?;
    let undefined = context.undefined();
    let api = context.call_rooted(factory, undefined, &operations)?;
    for name in [
        "eventNames",
        "listeners",
        "rawListeners",
        "listenerCount",
        "addListener",
        "prependListener",
        "once",
        "prependOnceListener",
        "removeListener",
        "removeAllListeners",
        "emit",
    ] {
        let value = get(context, api, name)?;
        let key = context.string_rooted(name);
        context.define_data_property_rooted(prototype, key, value, true, true, true)?;
    }
    for (source, alias) in [("addListener", "on"), ("removeListener", "off")] {
        let value = get(context, prototype, source)?;
        let key = context.string_rooted(alias);
        context.define_data_property_rooted(prototype, key, value, true, true, true)?;
    }
    Ok(())
}

pub(crate) fn names(
    context: &mut NativeContext<'_, NodeHost>,
    _: RootId,
    args: &[RootId],
) -> Result<RootId, RootedError> {
    let (emitter, _) = arguments(context, args);
    let count = get(context, emitter, "_eventsCount")?;
    let zero = context.number(0.0);
    if context.strict_equal_rooted(count, zero)? {
        return context.array_rooted(&[]);
    }
    let events = get(context, emitter, "_events")?;
    let mut names = Vec::new();
    for key in context.own_keys_rooted(events)? {
        let value = context.get_property_rooted(events, key)?;
        if !undefined(context, value) {
            names.push(key);
        }
    }
    context.array_rooted(&names)
}

pub(crate) fn raw_listeners(
    context: &mut NativeContext<'_, NodeHost>,
    _: RootId,
    args: &[RootId],
) -> Result<RootId, RootedError> {
    let (emitter, event) = arguments(context, args);
    let record = record(context, emitter, event)?;
    let entries = entries(context, record)?;
    context.array_rooted(&entries)
}

pub(crate) fn listeners(
    context: &mut NativeContext<'_, NodeHost>,
    _: RootId,
    args: &[RootId],
) -> Result<RootId, RootedError> {
    let (emitter, event) = arguments(context, args);
    let record = record(context, emitter, event)?;
    let mut entries = entries(context, record)?;
    for listener in &mut entries {
        let original = get(context, *listener, "listener")?;
        if context.truthy_rooted(original)? {
            *listener = original;
        }
    }
    context.array_rooted(&entries)
}

pub(crate) fn count(
    context: &mut NativeContext<'_, NodeHost>,
    _: RootId,
    args: &[RootId],
) -> Result<RootId, RootedError> {
    let (emitter, event) = arguments(context, args);
    let record = record(context, emitter, event)?;
    if undefined(context, record) {
        return Ok(context.number(0.0));
    }
    let undefined = context.undefined();
    let listener = args.get(2).copied().unwrap_or(undefined);
    let filtered = !context
        .rooted_value(listener)
        .is_some_and(|value| value.is_null() || value.is_undefined());
    if !filtered {
        return if context.is_callable_rooted(record)? {
            Ok(context.number(1.0))
        } else {
            get(context, record, "length")
        };
    }
    let mut count = 0;
    for entry in entries(context, record)? {
        if context.strict_equal_rooted(entry, listener)? {
            count += 1;
            continue;
        }
        let original = get(context, entry, "listener")?;
        if context.strict_equal_rooted(original, listener)? {
            count += 1;
        }
    }
    Ok(context.number(count as f64))
}

fn listener_arguments(
    context: &mut NativeContext<'_, NodeHost>,
    arguments: RootId,
) -> Result<Vec<RootId>, RootedError> {
    let length = get(context, arguments, "length")?;
    let length = context
        .rooted_value(length)
        .and_then(|value| value.as_number())
        .unwrap_or_default() as usize;
    (0..length)
        .map(|index| get(context, arguments, &index.to_string()))
        .collect()
}

/// The emitting counter belongs to the listener array, including nested emits.
pub(crate) fn emit_listeners(
    context: &mut NativeContext<'_, NodeHost>,
    _: RootId,
    args: &[RootId],
) -> Result<RootId, RootedError> {
    let missing = context.undefined();
    let target = args.first().copied().unwrap_or(missing);
    let event = args.get(1).copied().unwrap_or(missing);
    let arguments = args.get(2).copied().unwrap_or(missing);
    let record = args.get(3).copied().unwrap_or(missing);
    if undefined(context, record) {
        return Ok(context.boolean(false));
    }
    let data = context.host_function_data()?;
    let arguments_values = listener_arguments(context, arguments)?;
    if context.is_callable_rooted(record)? {
        let result = context.call_rooted(record, target, &arguments_values)?;
        super::event_constructor_shared_vm::capture_result(
            context, data, target, result, event, arguments,
        )?;
        return Ok(context.boolean(true));
    }
    let key = get(context, data, "emittingKey")?;
    let count = context.get_property_rooted(record, key)?;
    let next = context.numeric_update_rooted(count, false)?;
    context.assign_property_rooted(record, key, next)?;
    let result: Result<(), RootedError> = (|| {
        let mut index = 0;
        loop {
            let length = get(context, record, "length")?;
            let position = context.number(index as f64);
            if !context.greater_than_rooted(length, position)? {
                break;
            }
            let listener = get(context, record, &index.to_string())?;
            let result = context.call_rooted(listener, target, &arguments_values)?;
            super::event_constructor_shared_vm::capture_result(
                context, data, target, result, event, arguments,
            )?;
            index += 1;
        }
        Ok(())
    })();
    // Like a JS finally block, a failing counter restoration supersedes the call.
    let cleanup: Result<(), RootedError> = (|| {
        let count = context.get_property_rooted(record, key)?;
        let next = context.numeric_update_rooted(count, true)?;
        context.assign_property_rooted(record, key, next)
    })();
    if let Err(cleanup) = cleanup {
        if let Err(error) = result {
            if let Some(exception) = error.exception {
                context.release_root(exception);
            }
        }
        return Err(cleanup);
    }
    result?;
    Ok(context.boolean(true))
}

fn mutable_record(
    context: &mut NativeContext<'_, NodeHost>,
    data: RootId,
    events: RootId,
    event: RootId,
    record: RootId,
) -> Result<RootId, RootedError> {
    
    let key = get(context, data, "emittingKey")?;
    let emitting = context.get_property_rooted(record, key)?;
    let zero = context.number(0.0);
    if !context.greater_than_rooted(emitting, zero)? {
        return Ok(record);
    }
    let values = entries(context, record)?;
    let copy = context.array_rooted(&values)?;
    context.assign_property_rooted(copy, key, zero)?;
    let warned = get(context, record, "warned")?;
    if context.truthy_rooted(warned)? {
        let yes = context.boolean(true);
        context.assign_named_property_rooted(copy, "warned", yes)?;
    }
    context.assign_property_rooted(events, event, copy)?;
    Ok(copy)
}

enum RemovedListener {
    Record { record: RootId, requested: RootId },
    Argument(RootId),
}

fn notify_removed(
    context: &mut NativeContext<'_, NodeHost>,
    target: RootId,
    events: RootId,
    event: RootId,
    listener: RemovedListener,
) -> Result<(), RootedError> {
    let notification = get(context, events, "removeListener")?;
    if !undefined(context, notification) {
        let listener = match listener {
            RemovedListener::Record { record, requested } => {
                let original = get(context, record, "listener")?;
                if context.truthy_rooted(original)? {
                    original
                } else {
                    requested
                }
            }
            RemovedListener::Argument(listener) => listener,
        };
        let emit = get(context, target, "emit")?;
        let name = context.string_rooted("removeListener");
        context.call_rooted(emit, target, &[name, event, listener])?;
    }
    Ok(())
}

/// Remove the most recent matching callback, retaining other identical entries.
pub(crate) fn remove_listener(
    context: &mut NativeContext<'_, NodeHost>,
    _: RootId,
    args: &[RootId],
) -> Result<RootId, RootedError> {
    use super::event_constructor_shared_vm::event_removed;
    let missing = context.undefined();
    let target = args.first().copied().unwrap_or(missing);
    let event = args.get(1).copied().unwrap_or(missing);
    let listener = args.get(2).copied().unwrap_or(missing);
    validate_listener(context, listener)?;
    let events = get(context, target, "_events")?;
    if undefined(context, events) {
        return Ok(target);
    }
    let record = context.get_property_rooted(events, event)?;
    if undefined(context, record) {
        return Ok(target);
    }
    let data = context.host_function_data()?;
    let matches = if context.strict_equal_rooted(record, listener)? {
        true
    } else {
        let original = get(context, record, "listener")?;
        context.strict_equal_rooted(original, listener)?
    };
    if matches {
        event_removed(context, data, target, events, event)?;
        notify_removed(
            context,
            target,
            events,
            event,
            RemovedListener::Record {
                record,
                requested: listener,
            },
        )?;
        return Ok(target);
    }
    if context.is_callable_rooted(record)? {
        return Ok(target);
    }
    let record = mutable_record(context, data, events, event, record)?;
    let length = get(context, record, "length")?;
    let length = context
        .rooted_value(length)
        .and_then(|value| value.as_number())
        .unwrap_or_default() as usize;
    let mut position = None;
    for index in (0..length).rev() {
        let current = get(context, record, &index.to_string())?;
        if context.strict_equal_rooted(current, listener)? {
            position = Some(index);
            break;
        }
        let original = get(context, current, "listener")?;
        if context.strict_equal_rooted(original, listener)? {
            position = Some(index);
            break;
        }
    }
    let Some(position) = position else {
        return Ok(target);
    };
    if position == 0 {
        let shift = get(context, record, "shift")?;
        context.call_rooted(shift, record, &[])?;
    } else {
        let mut index = position;
        loop {
            let length = get(context, record, "length")?;
            let next_index = context.number((index + 1) as f64);
            if !context.greater_than_rooted(length, next_index)? {
                break;
            }
            let next = get(context, record, &(index + 1).to_string())?;
            context.assign_named_property_rooted(record, &index.to_string(), next)?;
            index += 1;
        }
        let pop = get(context, record, "pop")?;
        context.call_rooted(pop, record, &[])?;
    }
    let length = get(context, record, "length")?;
    let one = context.number(1.0);
    if context.strict_equal_rooted(length, one)? {
        let current = get(context, record, "0")?;
        context.assign_property_rooted(events, event, current)?;
    }
    notify_removed(
        context,
        target,
        events,
        event,
        RemovedListener::Argument(listener),
    )?;
    Ok(target)
}

fn reset_listener_store(
    context: &mut NativeContext<'_, NodeHost>,
    data: RootId,
    target: RootId,
) -> Result<(), RootedError> {
    
    let events = context.null_object_rooted()?;
    context.assign_named_property_rooted(target, "_events", events)?;
    let zero = context.number(0.0);
    context.assign_named_property_rooted(target, "_eventsCount", zero)?;
    let key = get(context, data, "shapeKey")?;
    let no = context.boolean(false);
    context.assign_property_rooted(target, key, no)
}

/// Bulk removal has its own numeric update rule and public notification order.
pub(crate) fn remove_all_listeners(
    context: &mut NativeContext<'_, NodeHost>,
    _: RootId,
    args: &[RootId],
) -> Result<RootId, RootedError> {
    
    let missing = context.undefined();
    let target = args.first().copied().unwrap_or(missing);
    let event = args.get(1).copied().unwrap_or(missing);
    let provided = args.get(2).copied().unwrap_or(missing);
    let provided = context.truthy_rooted(provided)?;
    let events = get(context, target, "_events")?;
    if undefined(context, events) {
        return Ok(target);
    }
    let data = context.host_function_data()?;
    let notification = get(context, events, "removeListener")?;
    if undefined(context, notification) {
        if !provided {
            reset_listener_store(context, data, target)?;
            return Ok(target);
        }
        let record = context.get_property_rooted(events, event)?;
        if !undefined(context, record) {
            let count = get(context, target, "_eventsCount")?;
            let next = context.numeric_update_rooted(count, true)?;
            context.assign_named_property_rooted(target, "_eventsCount", next)?;
            let zero = context.number(0.0);
            if context.strict_equal_rooted(next, zero)? {
                let fresh = context.null_object_rooted()?;
                context.assign_named_property_rooted(target, "_events", fresh)?;
            } else {
                let delete = get(context, data, "delete")?;
                context.call_rooted(delete, missing, &[events, event])?;
            }
        }
        let key = get(context, data, "shapeKey")?;
        let no = context.boolean(false);
        context.assign_property_rooted(target, key, no)?;
        return Ok(target);
    }
    if !provided {
        let name = context.string_rooted("removeListener");
        for key in context.own_keys_rooted(events)? {
            if context.strict_equal_rooted(key, name)? {
                continue;
            }
            let remove_all = get(context, target, "removeAllListeners")?;
            context.call_rooted(remove_all, target, &[key])?;
        }
        let remove_all = get(context, target, "removeAllListeners")?;
        context.call_rooted(remove_all, target, &[name])?;
        reset_listener_store(context, data, target)?;
        return Ok(target);
    }
    let record = context.get_property_rooted(events, event)?;
    if context.is_callable_rooted(record)? {
        let remove = get(context, target, "removeListener")?;
        context.call_rooted(remove, target, &[event, record])?;
    } else if !undefined(context, record) {
        let length = get(context, record, "length")?;
        let length = context
            .rooted_value(length)
            .and_then(|value| value.as_number())
            .unwrap_or_default() as usize;
        for index in (0..length).rev() {
            let listener = get(context, record, &index.to_string())?;
            let remove = get(context, target, "removeListener")?;
            context.call_rooted(remove, target, &[event, listener])?;
        }
    }
    Ok(target)
}

fn validate_listener(
    context: &mut NativeContext<'_, NodeHost>,
    listener: RootId,
) -> Result<(), RootedError> {
    if context.is_callable_rooted(listener)? {
        return Ok(());
    }
    Err(super::util_shared_vm::argument_type_error(
        context, listener, "listener", "function",
    )?)
}

/// Registration mutates an idle listener array and copies only an active array.
pub(crate) fn add_listener(
    context: &mut NativeContext<'_, NodeHost>,
    _: RootId,
    args: &[RootId],
) -> Result<RootId, RootedError> {
    
    let missing = context.undefined();
    let target = args.first().copied().unwrap_or(missing);
    let event = args.get(1).copied().unwrap_or(missing);
    let listener = args.get(2).copied().unwrap_or(missing);
    let prepend = args.get(3).copied().unwrap_or(missing);
    let prepend = context.truthy_rooted(prepend)?;
    validate_listener(context, listener)?;
    let data = context.host_function_data()?;
    let mut events = get(context, target, "_events")?;
    if undefined(context, events) {
        events = context.null_object_rooted()?;
        context.assign_named_property_rooted(target, "_events", events)?;
        let zero = context.number(0.0);
        context.assign_named_property_rooted(target, "_eventsCount", zero)?;
    } else {
        let notification = get(context, events, "newListener")?;
        if !undefined(context, notification) {
            let original = get(context, listener, "listener")?;
            let payload = if context
                .rooted_value(original)
                .is_some_and(|value| value.is_null() || value.is_undefined())
            {
                listener
            } else {
                original
            };
            let emit = get(context, target, "emit")?;
            let name = context.string_rooted("newListener");
            context.call_rooted(emit, target, &[name, event, payload])?;
            events = get(context, target, "_events")?;
        }
    }
    let previous = context.get_property_rooted(events, event)?;
    let current = if undefined(context, previous) {
        context.assign_property_rooted(events, event, listener)?;
        listener
    } else if context.is_callable_rooted(previous)? {
        let values = if prepend {
            [listener, previous]
        } else {
            [previous, listener]
        };
        let current = context.array_rooted(&values)?;
        let key = get(context, data, "emittingKey")?;
        let zero = context.number(0.0);
        context.assign_property_rooted(current, key, zero)?;
        context.assign_property_rooted(events, event, current)?;
        current
    } else {
        let current = mutable_record(context, data, events, event, previous)?;
        let append = get(context, current, if prepend { "unshift" } else { "push" })?;
        context.call_rooted(append, current, &[listener])?;
        current
    };
    super::event_limits_shared_vm::registered(context, data, target, event, previous, current)?;
    Ok(target)
}

/// Once's fired bit is private to its wrapper and changes before public removal.
pub(crate) fn once_invoke(
    context: &mut NativeContext<'_, NodeHost>,
    _: RootId,
    args: &[RootId],
) -> Result<RootId, RootedError> {
    let state = context.host_function_data()?;
    let fired = get(context, state, "fired")?;
    if context.truthy_rooted(fired)? {
        return Ok(context.undefined());
    }
    let yes = context.boolean(true);
    context.assign_named_property_rooted(state, "fired", yes)?;
    let target = get(context, state, "target")?;
    let event = get(context, state, "type")?;
    let listener = get(context, state, "listener")?;
    let wrapper = get(context, state, "wrapper")?;
    let remove = get(context, target, "removeListener")?;
    context.call_rooted(remove, target, &[event, wrapper])?;
    context.call_rooted(listener, target, args)
}

pub(crate) fn once(
    context: &mut NativeContext<'_, NodeHost>,
    _: RootId,
    args: &[RootId],
) -> Result<RootId, RootedError> {
    let missing = context.undefined();
    let target = args.first().copied().unwrap_or(missing);
    let event = args.get(1).copied().unwrap_or(missing);
    let listener = args.get(2).copied().unwrap_or(missing);
    let prepend = args.get(3).copied().unwrap_or(missing);
    let prepend = context.truthy_rooted(prepend)?;
    validate_listener(context, listener)?;
    let data = context.host_function_data()?;
    let no = context.boolean(false);
    let state = context.null_object_rooted()?;
    for (name, value) in [
        ("api", data),
        ("target", target),
        ("type", event),
        ("listener", listener),
        ("fired", no),
    ] {
        context.assign_named_property_rooted(state, name, value)?;
    }
    let invoke = context.host_function_with_data(
        crate::host::shared_vm::operation("emitterOnceInvoke"),
        state,
    )?;
    let factory = get(context, data, "onceFactory")?;
    let wrapper = context.call_rooted(factory, missing, &[invoke])?;
    context.assign_named_property_rooted(state, "wrapper", wrapper)?;
    context.assign_named_property_rooted(wrapper, "listener", listener)?;
    let add = get(
        context,
        target,
        if prepend { "prependListener" } else { "on" },
    )?;
    context.call_rooted(add, target, &[event, wrapper])?;
    Ok(target)
}

/// Dispatch from the canonical store; domain forwarding preserves primitive errors.
pub(crate) fn emit(
    context: &mut NativeContext<'_, NodeHost>,
    _: RootId,
    args: &[RootId],
) -> Result<RootId, RootedError> {
    let missing = context.undefined();
    let target = args.first().copied().unwrap_or(missing);
    let event = args.get(1).copied().unwrap_or(missing);
    let arguments = args.get(2).copied().unwrap_or(missing);
    let error = context.string_rooted("error");
    if !context.strict_equal_rooted(event, error)? {
        let events = get(context, target, "_events")?;
        if undefined(context, events) {
            return Ok(context.boolean(false));
        }
        let record = context.get_property_rooted(events, event)?;
        return emit_listeners(context, missing, &[target, event, arguments, record]);
    }
    let data = context.host_function_data()?;
    let events = get(context, target, "_events")?;
    if !undefined(context, events) {
        let monitor_key = get(context, data, "monitorKey")?;
        let monitor = context.get_property_rooted(events, monitor_key)?;
        if !undefined(context, monitor) {
            // Node dispatches through the public emit hook, including overrides.
            let public_emit = get(context, target, "emit")?;
            let mut values = vec![monitor_key];
            values.extend(listener_arguments(context, arguments)?);
            context.call_rooted(public_emit, target, &values)?;
        }
    }
    let listeners = if undefined(context, events) {
        missing
    } else {
        context.get_property_rooted(events, event)?
    };
    if undefined(context, listeners) {
        let domain = get(context, target, "domain")?;
        let has_domain = !context
            .rooted_value(domain)
            .is_some_and(|value| value.is_null() || value.is_undefined());
        let process = context.host_mut().shared_state().borrow().process_module;
        let is_process = if let Some(process) = process {
            context.strict_equal_rooted(target, process)?
        } else {
            false
        };
        if has_domain && !is_process {
            let values = listener_arguments(context, arguments)?;
            let provided = values.first().copied().unwrap_or(missing);
            let error = if context.truthy_rooted(provided)? {
                provided
            } else {
                let error = context.error_rooted("Unhandled error.")?;
                let code = context.string_rooted("ERR_UNHANDLED_ERROR");
                context.assign_named_property_rooted(error, "code", code)?;
                error
            };
            if context.is_object_rooted(error)? {
                context.assign_named_property_rooted(error, "domainEmitter", target)?;
                let domain_key = context.string_rooted("domain");
                context
                    .define_data_property_rooted(error, domain_key, domain, true, false, true)?;
                let no = context.boolean(false);
                context.assign_named_property_rooted(error, "domainThrown", no)?;
            }
            let domain_emit = get(context, domain, "emit")?;
            context.call_rooted(domain_emit, domain, &[event, error])?;
            return Ok(context.boolean(true));
        }
    }
    let bridge = get(context, data, "emitBridge")?;
    context.call_rooted(bridge, target, &[event, arguments, listeners])
}
