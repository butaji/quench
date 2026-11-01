//! EventEmitter inspection derives copies and counts from its listener store.

use crate::host::NodeHost;
use quench_runtime::{NativeContext, RootId, RootedError};


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
    for (name, operation, length) in PROTOTYPE_METHODS {
        let method = context.host_function_named_with_data(crate::host::shared_vm::operation(operation), name, data)?;
        let _ = length;
        let key = context.string_rooted(name);
        context.define_data_property_rooted(prototype, key, method, true, true, true)?;
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
    context.array_like_values_rooted(arguments)
}

/// The retained string `"error"` (`SharedNodeState::error_event_name`).
fn error_event_name(context: &mut NativeContext<'_, NodeHost>) -> Result<RootId, RootedError> {
    let shared = context.host_mut().shared_state();
    if let Some(name) = shared.borrow().error_event_name {
        return Ok(name);
    }
    let name = context.string_rooted("error");
    let name = context.retain(name)?;
    shared.borrow_mut().error_event_name = Some(name);
    Ok(name)
}

/// A single listener's result from the JavaScript `emit`: captured rejections apply to it as
/// `emit_listeners` applies them (`capture_result`).
pub(crate) fn capture_listener_result(
    context: &mut NativeContext<'_, NodeHost>,
    _: RootId,
    args: &[RootId],
) -> Result<RootId, RootedError> {
    let missing = context.undefined();
    let [target, result, event, arguments] =
        std::array::from_fn(|index| args.get(index).copied().unwrap_or(missing));
    let data = context.host_function_data()?;
    super::event_constructor_shared_vm::capture_result(context, data, target, result, event, arguments)?;
    Ok(missing)
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
    // Named as Node's bound wrapper reads.
    let wrapper = context.host_function_named_with_data(
        crate::host::shared_vm::operation("emitterOnceInvoke"),
        "bound onceWrapper",
        state,
    )?;
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

/// Dispatch from the canonical store.
pub(crate) fn emit(
    context: &mut NativeContext<'_, NodeHost>,
    _: RootId,
    args: &[RootId],
) -> Result<RootId, RootedError> {
    let missing = context.undefined();
    let target = args.first().copied().unwrap_or(missing);
    let event = args.get(1).copied().unwrap_or(missing);
    let arguments = args.get(2).copied().unwrap_or(missing);
    let error = error_event_name(context)?;
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
    unhandled_or_dispatch(context, target, event, arguments, listeners)
}

/// Node's `emit` for an event with no listener left to take it: an `'error'` event that no
/// listener (or only internal listeners of a destroyed stream) handles throws its error, or
/// `ERR_UNHANDLED_ERROR` describing a non-Error value; anything else dispatches to its
/// listeners.
fn unhandled_or_dispatch(
    context: &mut NativeContext<'_, NodeHost>,
    target: RootId,
    event: RootId,
    arguments: RootId,
    listeners: RootId,
) -> Result<RootId, RootedError> {
    let values = if undefined(context, listeners) {
        Vec::new()
    } else if context.is_array_rooted(listeners)? {
        entries(context, listeners)?
    } else {
        vec![listeners]
    };
    let error_event = error_event_name(context)?;
    if !context.strict_equal_rooted(event, error_event)? {
        return emit_listeners(context, target, &[target, event, arguments, listeners]);
    }
    let mut internal_only = !values.is_empty();
    for listener in &values {
        let flag = get(context, *listener, "__quenchInternal")?;
        let yes = context.boolean(true);
        if !context.strict_equal_rooted(flag, yes)? {
            internal_only = false;
            break;
        }
    }
    let owner_destroyed = if internal_only {
        let owner_key = symbol_for(context, STREAM_OWNER)?;
        let owner = context.get_property_rooted(target, owner_key)?;
        if context.is_object_rooted(owner)? {
            let destroyed = get(context, owner, "destroyed")?;
            let yes = context.boolean(true);
            context.strict_equal_rooted(destroyed, yes)?
        } else {
            false
        }
    } else {
        false
    };
    if !values.is_empty() && !(internal_only && owner_destroyed) {
        return emit_listeners(context, target, &[target, event, arguments, listeners]);
    }
    let call_arguments = if undefined(context, arguments) { Vec::new() } else { entries(context, arguments)? };
    if internal_only {
        for listener in values {
            if context.is_callable_rooted(listener)? {
                context.call_rooted(listener, target, &call_arguments)?;
            }
        }
    }
    let missing = context.undefined();
    let error = call_arguments.first().copied().unwrap_or(missing);
    if context.is_error_rooted(error)? {
        return Err(context.throw(error));
    }
    let detail = super::console_format_shared_vm::inspect(context, error, Default::default())?;
    let unhandled = context.error_rooted(&format!("Unhandled error. ({detail})"))?;
    let code = context.string_rooted("ERR_UNHANDLED_ERROR");
    context.assign_named_property_rooted(unhandled, "code", code)?;
    context.assign_named_property_rooted(unhandled, "context", error)?;
    Err(context.throw(unhandled))
}

/// The registry symbol a stream stores its owning stream under.
const STREAM_OWNER: &str = "quench.internal.streamOwner";
/// `EventEmitter.captureRejectionSymbol`.
const REJECTION_SYMBOL: &str = "nodejs.rejection";

fn symbol_for(context: &mut NativeContext<'_, NodeHost>, description: &str) -> Result<RootId, RootedError> {
    let global = context.global_root()?;
    let symbol = get(context, global, "Symbol")?;
    let registry = get(context, symbol, "for")?;
    let description = context.string_rooted(description);
    context.call_rooted(registry, symbol, &[description])
}

/// `EventEmitter.prototype`'s methods, as `(name, operation, length)`; each passes `this`
/// and its arguments to its operation.
const PROTOTYPE_METHODS: &[(&str, &str, u32)] = &[
    ("eventNames", "emitterProtoEventNames", 0),
    ("listeners", "emitterProtoListeners", 1),
    ("rawListeners", "emitterProtoRawListeners", 1),
    ("listenerCount", "emitterProtoListenerCount", 2),
    ("addListener", "emitterProtoAddListener", 2),
    ("prependListener", "emitterProtoPrependListener", 2),
    ("once", "emitterProtoOnce", 2),
    ("prependOnceListener", "emitterProtoPrependOnceListener", 2),
    ("removeListener", "emitterProtoRemoveListener", 2),
    ("removeAllListeners", "emitterProtoRemoveAllListeners", 1),
    ("emit", "emitterProtoEmit", 1),
];

fn argument(context: &mut NativeContext<'_, NodeHost>, args: &[RootId], index: usize) -> RootId {
    match args.get(index) {
        Some(value) => *value,
        None => context.undefined(),
    }
}

pub(crate) fn proto_event_names(c: &mut NativeContext<'_, NodeHost>, this: RootId, _: &[RootId]) -> Result<RootId, RootedError> {
    let missing = c.undefined();
    names(c, missing, &[this])
}

pub(crate) fn proto_listeners(c: &mut NativeContext<'_, NodeHost>, this: RootId, args: &[RootId]) -> Result<RootId, RootedError> {
    let (missing, event) = (c.undefined(), argument(c, args, 0));
    listeners(c, missing, &[this, event])
}

pub(crate) fn proto_raw_listeners(c: &mut NativeContext<'_, NodeHost>, this: RootId, args: &[RootId]) -> Result<RootId, RootedError> {
    let (missing, event) = (c.undefined(), argument(c, args, 0));
    raw_listeners(c, missing, &[this, event])
}

pub(crate) fn proto_listener_count(c: &mut NativeContext<'_, NodeHost>, this: RootId, args: &[RootId]) -> Result<RootId, RootedError> {
    let (missing, event, listener) = (c.undefined(), argument(c, args, 0), argument(c, args, 1));
    count(c, missing, &[this, event, listener])
}

fn proto_add(c: &mut NativeContext<'_, NodeHost>, this: RootId, args: &[RootId], prepend: bool) -> Result<RootId, RootedError> {
    let (missing, event, listener) = (c.undefined(), argument(c, args, 0), argument(c, args, 1));
    let prepend = c.boolean(prepend);
    add_listener(c, missing, &[this, event, listener, prepend])
}

pub(crate) fn proto_add_listener(c: &mut NativeContext<'_, NodeHost>, this: RootId, args: &[RootId]) -> Result<RootId, RootedError> {
    proto_add(c, this, args, false)
}

pub(crate) fn proto_prepend_listener(c: &mut NativeContext<'_, NodeHost>, this: RootId, args: &[RootId]) -> Result<RootId, RootedError> {
    proto_add(c, this, args, true)
}

fn proto_once_with(c: &mut NativeContext<'_, NodeHost>, this: RootId, args: &[RootId], prepend: bool) -> Result<RootId, RootedError> {
    let (missing, event, listener) = (c.undefined(), argument(c, args, 0), argument(c, args, 1));
    let prepend = c.boolean(prepend);
    once(c, missing, &[this, event, listener, prepend])
}

pub(crate) fn proto_once(c: &mut NativeContext<'_, NodeHost>, this: RootId, args: &[RootId]) -> Result<RootId, RootedError> {
    proto_once_with(c, this, args, false)
}

pub(crate) fn proto_prepend_once_listener(c: &mut NativeContext<'_, NodeHost>, this: RootId, args: &[RootId]) -> Result<RootId, RootedError> {
    proto_once_with(c, this, args, true)
}

pub(crate) fn proto_remove_listener(c: &mut NativeContext<'_, NodeHost>, this: RootId, args: &[RootId]) -> Result<RootId, RootedError> {
    let (missing, event, listener) = (c.undefined(), argument(c, args, 0), argument(c, args, 1));
    remove_listener(c, missing, &[this, event, listener])
}

pub(crate) fn proto_remove_all_listeners(c: &mut NativeContext<'_, NodeHost>, this: RootId, args: &[RootId]) -> Result<RootId, RootedError> {
    let (missing, event) = (c.undefined(), argument(c, args, 0));
    let has_event = c.boolean(!args.is_empty());
    remove_all_listeners(c, missing, &[this, event, has_event])
}

pub(crate) fn proto_emit(c: &mut NativeContext<'_, NodeHost>, this: RootId, args: &[RootId]) -> Result<RootId, RootedError> {
    let (missing, event) = (c.undefined(), argument(c, args, 0));
    let rest = c.array_rooted(args.get(1..).unwrap_or(&[]))?;
    emit(c, missing, &[this, event, rest])
}

/// `EventEmitter` (callable as `EventEmitter.call(this)` and with `new`): runs
/// `EventEmitter.init` on the receiver, whatever function that property holds (`node:domain`
/// replaces it).
pub(crate) fn constructor(c: &mut NativeContext<'_, NodeHost>, this: RootId, args: &[RootId]) -> Result<RootId, RootedError> {
    let data = c.host_function_data()?;
    let emitter = get(c, data, "emitter")?;
    let init = get(c, emitter, "init")?;
    c.call_rooted(init, this, args)?;
    Ok(c.undefined())
}

/// `EventEmitter.init(options)`: a fresh listener store unless the receiver inherits one it
/// already shadows, `_maxListeners` kept or `undefined`, and the capture-rejections flag.
pub(crate) fn init(c: &mut NativeContext<'_, NodeHost>, this: RootId, args: &[RootId]) -> Result<RootId, RootedError> {
    let data = c.host_function_data()?;
    let shape_key = get(c, data, "shapeKey")?;
    let capture_key = get(c, data, "captureKey")?;
    let events = get(c, this, "_events")?;
    let prototype = c.prototype_rooted(this)?;
    let inherited = if c.is_object_rooted(prototype)? { get(c, prototype, "_events")? } else { c.undefined() };
    let fresh = undefined(c, events) || c.strict_equal_rooted(events, inherited)?;
    if fresh {
        let store = c.null_object_rooted()?;
        c.assign_named_property_rooted(this, "_events", store)?;
        let zero = c.number(0.0);
        c.assign_named_property_rooted(this, "_eventsCount", zero)?;
    }
    let shaped = c.boolean(!fresh);
    let missing = c.undefined();
    c.set_property_rooted(this, shape_key, shaped, this)?;
    let maximum = get(c, this, "_maxListeners")?;
    if !c.truthy_rooted(maximum)? {
        c.assign_named_property_rooted(this, "_maxListeners", missing)?;
    }
    let options = argument(c, args, 0);
    let capture = if c.is_object_rooted(options)? { get(c, options, "captureRejections")? } else { missing };
    let value = if c.truthy_rooted(capture)? {
        super::event_constructor_shared_vm::validate_capture_rejections(c, missing, &[capture])?;
        c.boolean(true)
    } else {
        let emitter = get(c, data, "emitter")?;
        let shared = get(c, emitter, "prototype")?;
        c.get_property_rooted(shared, capture_key)?
    };
    c.set_property_rooted(this, capture_key, value, this)?;
    Ok(missing)
}

/// Defines the shared `EventEmitter` class as `globalThis.__nodeEventEmitter`.
pub(crate) fn install_class(c: &mut NativeContext<'_, NodeHost>, data: RootId) -> Result<RootId, RootedError> {
    let emitter = c.host_constructor_named_with_data(crate::host::shared_vm::operation("emitterConstructor"), "EventEmitter", data)?;
    c.assign_named_property_rooted(data, "emitter", emitter)?;
    // Node assigns `init` an anonymous function.
    let init = c.host_function_named_with_data(crate::host::shared_vm::operation("emitterInit"), "", data)?;
    c.assign_named_property_rooted(emitter, "init", init)?;
    let rejection = symbol_for(c, REJECTION_SYMBOL)?;
    c.assign_named_property_rooted(emitter, "captureRejectionSymbol", rejection)?;
    let no = c.boolean(false);
    c.assign_named_property_rooted(emitter, "usingDomains", no)?;
    let global = c.global_root()?;
    let key = c.string_rooted("__nodeEventEmitter");
    c.define_data_property_rooted(global, key, emitter, false, false, true)?;
    Ok(emitter)
}
