//! Native abort listener registration, ordered dispatch and disposal.

use super::event_constructor_shared_vm::get;
use super::events_shared_vm::listener_records;
use crate::host::NodeHost;
use quench_runtime::{NativeContext, RootId, RootedError};

pub(crate) fn abort_add_event_listener(
    context: &mut NativeContext<'_, NodeHost>,
    _: RootId,
    args: &[RootId],
) -> Result<RootId, RootedError> {
    use super::event_constructor_shared_vm::named_set;
    let undefined = context.undefined();
    let signal = args.first().copied().unwrap_or(undefined);
    let state = args.get(1).copied().unwrap_or(undefined);
    let event = args.get(2).copied().unwrap_or(undefined);
    let listener = args.get(3).copied().unwrap_or(undefined);
    let options = args.get(4).copied().unwrap_or(undefined);
    let refresh = args.get(5).copied().unwrap_or(undefined);
    if context.string_text(event)?.as_deref() != Some("abort")
        || context
            .rooted_value(listener)
            .is_some_and(|v| v.is_null() || v.is_undefined())
    {
        return Ok(undefined);
    }
    if !context.is_callable_rooted(listener)? {
        let method = get(context, listener, "handleEvent")?;
        if !context.is_callable_rooted(method)? {
            return Ok(undefined);
        }
    }
    let records = get(context, state, "listeners")?;
    let entries = listener_records(context, records)?;
    for entry in &entries {
        let callback = get(context, *entry, "listener")?;
        if context.strict_equal_rooted(callback, listener)? {
            return Ok(undefined);
        }
    }
    let data = context.host_function_data()?;
    let entry = context.object_rooted()?;
    named_set(context, data, entry, "listener", listener)?;
    let options_present = !context
        .rooted_value(options)
        .is_some_and(|v| v.is_null() || v.is_undefined());
    let once = if options_present {
        get(context, options, "once")?
    } else {
        undefined
    };
    let once = context.truthy_rooted(once)?;
    let once = context.boolean(once);
    named_set(context, data, entry, "once", once)?;
    let resist_key = get(context, data, "resistKey")?;
    let resist = if options_present {
        context.get_property_rooted(options, resist_key)?
    } else {
        undefined
    };
    let resist = context.truthy_rooted(resist)?;
    let resist = context.boolean(resist);
    named_set(context, data, entry, "resistStopPropagation", resist)?;
    let key = context.string_rooted(&entries.len().to_string());
    context.define_data_property_rooted(records, key, entry, true, true, true)?;
    update_listener_count(context, data, state, false)?;
    context.call_rooted(refresh, undefined, &[signal, state])?;
    Ok(undefined)
}

fn invoke_abort_listener(
    context: &mut NativeContext<'_, NodeHost>,
    listener: RootId,
    signal: RootId,
    event: RootId,
) -> Result<(), RootedError> {
    if context.is_callable_rooted(listener)? {
        context.call_rooted(listener, signal, &[event])?;
    } else {
        let method = get(context, listener, "handleEvent")?;
        if context.is_callable_rooted(method)? {
            context.call_rooted(method, listener, &[event])?;
        }
    }
    Ok(())
}

pub(crate) fn abort_dispatch_listeners(
    context: &mut NativeContext<'_, NodeHost>,
    _: RootId,
    args: &[RootId],
) -> Result<RootId, RootedError> {
    let undefined = context.undefined();
    let signal = args.first().copied().unwrap_or(undefined);
    let state = args.get(1).copied().unwrap_or(undefined);
    let event = args.get(2).copied().unwrap_or(undefined);
    let remove = args.get(3).copied().unwrap_or(undefined);
    let records = get(context, state, "listeners")?;
    for entry in listener_records(context, records)? {
        let current = get(context, state, "listeners")?;
        let mut present = false;
        for candidate in listener_records(context, current)? {
            if context.strict_equal_rooted(candidate, entry)? {
                present = true;
                break;
            }
        }
        if !present {
            continue;
        }
        let stopped = get(context, event, "_quenchImmediatePropagationStopped")?;
        let resist = get(context, entry, "resistStopPropagation")?;
        if context.truthy_rooted(stopped)? && !context.truthy_rooted(resist)? {
            continue;
        }
        let listener = get(context, entry, "listener")?;
        let once = get(context, entry, "once")?;
        if context.truthy_rooted(once)? {
            context.call_rooted(remove, undefined, &[signal, state, listener])?;
        }
        invoke_abort_listener(context, listener, signal, event)?;
    }
    Ok(undefined)
}

pub(crate) fn add_abort_listener(
    context: &mut NativeContext<'_, NodeHost>,
    _: RootId,
    args: &[RootId],
) -> Result<RootId, RootedError> {
    let undefined = context.undefined();
    let signal = args.first().copied().unwrap_or(undefined);
    let listener = args.get(1).copied().unwrap_or(undefined);
    let aborted_key = context.string_rooted("aborted");
    let nullish = context
        .rooted_value(signal)
        .is_some_and(|v| v.is_null() || v.is_undefined());
    if nullish
        || !context.is_object_rooted(signal)?
        || !context.has_property_rooted(signal, aborted_key)?
    {
        return Err(super::util_shared_vm::argument_type_error(
            context,
            signal,
            "signal",
            "AbortSignal",
        )?);
    }
    if !context.is_callable_rooted(listener)? {
        return Err(super::util_shared_vm::argument_type_error(
            context, listener, "listener", "function",
        )?);
    }
    let data = context.host_function_data()?;
    let aborted = context.get_property_rooted(signal, aborted_key)?;
    let live = !context.truthy_rooted(aborted)?;
    if live {
        let options = get(context, data, "abortListenerOptions")?;
        let add = get(context, signal, "addEventListener")?;
        let event = context.string_rooted("abort");
        context.call_rooted(add, signal, &[event, listener, options])?;
    } else {
        let job = context.host_function_with_data(
            crate::host::shared_vm::operation("abortedListenerJob"),
            listener,
        )?;
        context.queue_microtask_rooted(job, &[])?;
    }
    let live = context.boolean(live);
    let dispose_data = context.array_rooted(&[signal, listener, live])?;
    let dispose = context.host_function_with_data(
        crate::host::shared_vm::operation("abortListenerDispose"),
        dispose_data,
    )?;
    let factory = get(context, data, "disposeFactory")?;
    let key = get(context, data, "disposeKey")?;
    context.call_rooted(factory, undefined, &[dispose, key])
}

pub(crate) fn aborted_listener_job(
    context: &mut NativeContext<'_, NodeHost>,
    _: RootId,
    _: &[RootId],
) -> Result<RootId, RootedError> {
    let listener = context.host_function_data()?;
    let undefined = context.undefined();
    context.call_rooted(listener, undefined, &[])?;
    Ok(undefined)
}

pub(crate) fn abort_listener_dispose(
    context: &mut NativeContext<'_, NodeHost>,
    _: RootId,
    _: &[RootId],
) -> Result<RootId, RootedError> {
    let data = context.host_function_data()?;
    let live = get(context, data, "2")?;
    let undefined = context.undefined();
    if context.truthy_rooted(live)? {
        let signal = get(context, data, "0")?;
        let listener = get(context, data, "1")?;
        let remove = get(context, signal, "removeEventListener")?;
        let event = context.string_rooted("abort");
        context.call_rooted(remove, signal, &[event, listener])?;
    }
    Ok(undefined)
}

pub(crate) fn abort_get_handler(
    context: &mut NativeContext<'_, NodeHost>,
    _: RootId,
    args: &[RootId],
) -> Result<RootId, RootedError> {
    let undefined = context.undefined();
    let state = args.first().copied().unwrap_or(undefined);
    let wrapper = get(context, state, "onabortWrapper")?;
    if !context.truthy_rooted(wrapper)? {
        return Ok(context.null());
    }
    let handler = get(context, wrapper, "handler")?;
    if context
        .rooted_value(handler)
        .is_some_and(|v| v.is_null() || v.is_undefined())
    {
        Ok(context.null())
    } else {
        Ok(handler)
    }
}

pub(crate) fn abort_set_handler(
    context: &mut NativeContext<'_, NodeHost>,
    _: RootId,
    args: &[RootId],
) -> Result<RootId, RootedError> {
    use super::event_constructor_shared_vm::named_set;
    let undefined = context.undefined();
    let signal = args.first().copied().unwrap_or(undefined);
    let state = args.get(1).copied().unwrap_or(undefined);
    let value = args.get(2).copied().unwrap_or(undefined);
    let refresh = args.get(3).copied().unwrap_or(undefined);
    let value = if context.is_callable_rooted(value)?
        || (context.is_object_rooted(value)?
            && !context.rooted_value(value).is_some_and(|v| v.is_null()))
    {
        value
    } else {
        context.null()
    };
    let data = context.host_function_data()?;
    let mut wrapper = get(context, state, "onabortWrapper")?;
    let new = !context.truthy_rooted(wrapper)?;
    if new {
        let holder = context.null_object_rooted()?;
        let invoke = context.host_function_with_data(
            crate::host::shared_vm::operation("abortInvokeHandler"),
            holder,
        )?;
        let factory = get(context, data, "abortHandlerFactory")?;
        wrapper = context.call_rooted(factory, undefined, &[invoke])?;
        named_set(context, data, holder, "wrapper", wrapper)?;
        named_set(context, data, state, "onabortWrapper", wrapper)?;
    }
    let records = get(context, state, "listeners")?;
    let root_exists = !listener_records(context, records)?.is_empty();
    if !new && root_exists {
        let previous = get(context, wrapper, "handler")?;
        if context.is_callable_rooted(previous)? {
            update_listener_count(context, data, state, true)?;
        }
    }
    named_set(context, data, wrapper, "handler", value)?;
    let event = context.string_rooted("abort");
    let callable = context.is_callable_rooted(value)?;
    if new || (callable && !root_exists) {
        abort_add_event_listener(
            context,
            undefined,
            &[signal, state, event, wrapper, undefined, refresh],
        )?;
    } else {
        if callable {
            update_listener_count(context, data, state, false)?;
        }
        context.call_rooted(refresh, undefined, &[signal, state])?;
    }
    Ok(undefined)
}

pub(crate) fn abort_invoke_handler(
    context: &mut NativeContext<'_, NodeHost>,
    _: RootId,
    args: &[RootId],
) -> Result<RootId, RootedError> {
    let undefined = context.undefined();
    let receiver = args.first().copied().unwrap_or(undefined);
    let values = args.get(1).copied().unwrap_or(undefined);
    let holder = context.host_function_data()?;
    let wrapper = get(context, holder, "wrapper")?;
    let handler = get(context, wrapper, "handler")?;
    if !context.is_callable_rooted(handler)? {
        return Ok(undefined);
    }
    let arguments = listener_records(context, values)?;
    context.call_rooted(handler, receiver, &arguments)
}

/// Node's onabort handler activation can change size without unlinking its wrapper.
fn update_listener_count(
    context: &mut NativeContext<'_, NodeHost>,
    data: RootId,
    state: RootId,
    decrement: bool,
) -> Result<(), RootedError> {
    let count = get(context, state, "listenerCount")?;
    let next = context.numeric_update_rooted(count, decrement)?;
    super::event_constructor_shared_vm::named_set(context, data, state, "listenerCount", next)
}

pub(crate) fn abort_remove_event_listener(
    context: &mut NativeContext<'_, NodeHost>,
    _: RootId,
    args: &[RootId],
) -> Result<RootId, RootedError> {
    let undefined = context.undefined();
    let signal = args.first().copied().unwrap_or(undefined);
    let state = args.get(1).copied().unwrap_or(undefined);
    let listener = args.get(2).copied().unwrap_or(undefined);
    let refresh = args.get(3).copied().unwrap_or(undefined);
    let data = context.host_function_data()?;
    let records = get(context, state, "listeners")?;
    let mut retained = Vec::new();
    for entry in listener_records(context, records)? {
        let callback = get(context, entry, "listener")?;
        if context.strict_equal_rooted(callback, listener)? {
            update_listener_count(context, data, state, true)?;
        } else {
            retained.push(entry);
        }
    }
    let records = context.array_rooted(&retained)?;
    super::event_constructor_shared_vm::named_set(context, data, state, "listeners", records)?;
    context.call_rooted(refresh, undefined, &[signal, state])?;
    Ok(undefined)
}
