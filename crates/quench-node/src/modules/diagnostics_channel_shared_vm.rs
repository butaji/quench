//! Shared-VM diagnostics surfaces exercised by the framework profile.
//!
//! Channel names are the keys and subscriber roots the values in
//! `DiagnosticsState`; tracing channels derive their event names from a prefix.

use crate::host::NodeHost;
use quench_runtime::{NativeContext, RootId, RootedError, Runtime};
use std::collections::HashMap;

#[derive(Default)]
pub(crate) struct SharedDiagnosticsState {
    pub(crate) channels: HashMap<String, Vec<RootId>>,
    pub(crate) module: Option<RootId>,
}

const TRACE_PREFIX: &str = "\0quench:shared-diagnostics:trace-prefix";
const TRACE_NAMES: [&str; 5] = ["start", "end", "asyncStart", "asyncEnd", "error"];

pub(crate) fn module(context: &mut NativeContext<'_, NodeHost>) -> Result<RootId, RootedError> {
    if let Some(module) = context
        .host_mut()
        .shared_state()
        .borrow()
        .diagnostics
        .module
    {
        return Ok(module);
    }
    let module = context.object_rooted()?;
    // The host's subscriber lists, which channels publish through and HTTP reads.
    let api = context.object_rooted()?;
    install_operation(context, api, "subscribe", "diagnosticsSubscribe")?;
    install_operation(context, api, "unsubscribe", "diagnosticsUnsubscribe")?;
    install_operation(context, api, "hasSubscribers", "diagnosticsHasSubscribers")?;
    install_operation(context, api, "publish", "diagnosticsPublish")?;
    let is_promise =
        context.host_function(crate::host::shared_vm::operation("diagnosticsIsPromise"))?;
    let factory = crate::host::removed_javascript("node:diagnostics_channel/channel.js")?;
    let trace_names = TRACE_NAMES
        .iter()
        .map(|name| context.string_rooted(name))
        .collect::<Vec<_>>();
    let trace_names = context.array_rooted(&trace_names)?;
    let primordials =
        crate::host::shared_vm::commonjs::node_primordials(context)?.ok_or_else(|| {
            RootedError::host("diagnostics channels require the configured SDK primordials")
        })?;
    let safe_map = property(context, primordials, "SafeMap")?;
    let safe_weak_map = property(context, primordials, "SafeWeakMap")?;
    let undefined = context.undefined();
    let channel_api = context.call_rooted(
        factory, undefined, &[api, trace_names, is_promise, safe_map, safe_weak_map],
    )?;
    for name in [
        "Channel",
        "BoundedChannel",
        "channel",
        "tracingChannel",
        "boundedChannel",
        "subscribe",
        "unsubscribe",
        "hasSubscribers",
    ] {
        let value = property(context, channel_api, name)?;
        set(context, module, name, value)?;
    }
    context.release_root(trace_names);
    context.release_root(api);
    context.release_root(channel_api);
    context.release_root(factory);
    let retained = context.retain(module)?;
    context
        .host_mut()
        .shared_state()
        .borrow_mut()
        .diagnostics
        .module = Some(retained);
    Ok(module)
}

pub(crate) fn is_promise(
    context: &mut NativeContext<'_, NodeHost>,
    _: RootId,
    args: &[RootId],
) -> Result<RootId, RootedError> {
    let Some(promise) = args.first().copied() else {
        return Ok(context.boolean(false));
    };
    let is_promise = context.is_promise_rooted(promise)?;
    Ok(context.boolean(is_promise))
}

pub(crate) fn subscribe(
    context: &mut NativeContext<'_, NodeHost>,
    _: RootId,
    args: &[RootId],
) -> Result<RootId, RootedError> {
    let name = string_arg(context, args.first().copied(), "name")?;
    let callback = args
        .get(1)
        .copied()
        .ok_or_else(|| invalid_callback(context))?;
    subscribe_named(context, &name, callback)?;
    Ok(context.undefined())
}

pub(crate) fn unsubscribe(
    context: &mut NativeContext<'_, NodeHost>,
    _: RootId,
    args: &[RootId],
) -> Result<RootId, RootedError> {
    let name = string_arg(context, args.first().copied(), "name")?;
    let callback = args
        .get(1)
        .copied()
        .ok_or_else(|| invalid_callback(context))?;
    if !context.is_callable_rooted(callback)? {
        return Err(invalid_callback(context));
    }
    let subscribers = context
        .host_mut()
        .shared_state()
        .borrow()
        .diagnostics
        .channels
        .get(&name)
        .cloned()
        .unwrap_or_default();
    let mut removed = None;
    for subscriber in subscribers {
        if context.same_value_rooted(subscriber, callback)? {
            removed = Some(subscriber);
            break;
        }
    }
    let removed = removed.and_then(|target| {
        let shared = context.host_mut().shared_state();
        let mut state = shared.borrow_mut();
        let channels = &mut state.diagnostics.channels;
        let mut empty = false;
        let removed = channels.get_mut(&name).and_then(|subscribers| {
            let removed = subscribers
                .iter()
                .position(|subscriber| *subscriber == target)
                .map(|index| subscribers.remove(index));
            empty = subscribers.is_empty();
            removed
        });
        if empty {
            channels.remove(&name);
        }
        removed
    });
    let did_remove = removed.is_some();
    if let Some(removed) = removed {
        context.release_root(removed);
    }
    Ok(context.boolean(did_remove))
}

pub(crate) fn has_subscribers(
    context: &mut NativeContext<'_, NodeHost>,
    _: RootId,
    args: &[RootId],
) -> Result<RootId, RootedError> {
    let name = string_arg(context, args.first().copied(), "name")?;
    let subscribed = channel_has_subscribers(&context.host_mut().shared_state().borrow(), &name);
    Ok(context.boolean(subscribed))
}

/// Whether channel `name` has a subscriber, which a publish delivers its message to.
pub(crate) fn channel_has_subscribers(state: &crate::host::SharedNodeState, name: &str) -> bool {
    state.diagnostics.channels.get(name).is_some_and(|subscribers| !subscribers.is_empty())
}


/// `REPORT_SUBSCRIBER_ERROR` of the value subscriber `exception` threw.
fn report_subscriber_error(context: &mut NativeContext<'_, NodeHost>, exception: RootId) -> Result<(), RootedError> {
    let report = crate::host::removed_javascript("node:diagnostics_channel/report.js")?;
    let undefined = context.undefined();
    let reported = context.call_rooted(report, undefined, &[exception])?;
    context.release_root(reported);
    Ok(())
}

pub(crate) fn publish(
    context: &mut NativeContext<'_, NodeHost>,
    _: RootId,
    args: &[RootId],
) -> Result<RootId, RootedError> {
    let name = string_arg(context, args.first().copied(), "name")?;
    let missing_message = args.get(1).is_none();
    let message = args.get(1).copied().unwrap_or_else(|| context.undefined());
    let name_value = context.string_rooted(&name);
    let callbacks = context
        .host_mut()
        .shared_state()
        .borrow()
        .diagnostics
        .channels
        .get(&name)
        .cloned()
        .unwrap_or_default();
    let mut first_error = None;
    for callback in callbacks {
        let undefined = context.undefined();
        let result = context.call_rooted(callback, undefined, &[message, name_value]);
        context.release_root(undefined);
        match result {
            Ok(result) => {
                context.release_root(result);
            }
            Err(RootedError { exception: Some(exception), .. }) => {
                report_subscriber_error(context, exception)?;
            }
            Err(error) => {
                if first_error.is_none() {
                    first_error = Some(error);
                }
            }
        }
    }
    context.release_root(name_value);
    if missing_message {
        context.release_root(message);
    }
    if let Some(error) = first_error {
        return Err(error);
    }
    Ok(context.undefined())
}

pub(crate) fn tracing_channel(
    context: &mut NativeContext<'_, NodeHost>,
    _: RootId,
    args: &[RootId],
) -> Result<RootId, RootedError> {
    let name = string_arg(context, args.first().copied(), "nameOrChannels")?;
    let tracing = context.object_rooted()?;
    let prefix = context.string_rooted(&name);
    set(context, tracing, TRACE_PREFIX, prefix)?;
    install_operation(context, tracing, "subscribe", "diagnosticsTracingSubscribe")?;
    install_operation(
        context,
        tracing,
        "traceCallback",
        "diagnosticsTraceCallback",
    )?;
    Ok(tracing)
}

pub(crate) fn tracing_subscribe(
    context: &mut NativeContext<'_, NodeHost>,
    receiver: RootId,
    args: &[RootId],
) -> Result<RootId, RootedError> {
    let prefix = trace_prefix(context, receiver)?;
    let handlers = args.first().copied().unwrap_or_else(|| context.undefined());
    for event in TRACE_NAMES {
        let callback = property(context, handlers, event)?;
        if !is_undefined(context, callback) {
            subscribe_named(context, &trace_name(&prefix, event), callback)?;
        }
    }
    Ok(context.undefined())
}

pub(crate) fn trace_callback(
    context: &mut NativeContext<'_, NodeHost>,
    receiver: RootId,
    args: &[RootId],
) -> Result<RootId, RootedError> {
    let Some(function) = args.first().copied() else {
        return Err(invalid_arg(context, "fn"));
    };
    if !context.is_callable_rooted(function)? {
        return Err(invalid_arg(context, "fn"));
    }
    let prefix = trace_prefix(context, receiver)?;
    let call_args = args.get(4..).unwrap_or_default();
    let callback_index = callback_index(context, args.get(1).copied(), call_args.len());
    let callback = callback_index.and_then(|index| call_args.get(index).copied());
    let Some(callback) =
        callback.filter(|callback| context.is_callable_rooted(*callback).unwrap_or(false))
    else {
        return Err(invalid_callback(context));
    };
    let this_arg = args.get(3).copied().unwrap_or_else(|| context.undefined());
    if !has_tracing_subscribers(context, &prefix) {
        return context.call_rooted(function, this_arg, call_args);
    }

    let event_context = match args.get(2).copied() {
        Some(context) => context,
        None => context.object_rooted()?,
    };
    publish_named(context, &trace_name(&prefix, "start"), event_context)?;
    let wrapper = callback_wrapper(context, &prefix, event_context, callback)?;
    let Some(index) = callback_index else {
        return Err(invalid_callback(context));
    };
    let mut wrapped_args = call_args.to_vec();
    wrapped_args[index] = wrapper;
    let result = context.call_rooted(function, this_arg, &wrapped_args);
    if result.is_ok() {
        publish_named(context, &trace_name(&prefix, "end"), event_context)?;
    }
    result
}

pub(crate) fn trace_complete(
    context: &mut NativeContext<'_, NodeHost>,
    _: RootId,
    args: &[RootId],
) -> Result<RootId, RootedError> {
    let [this_arg, prefix_root, event_context, callback, callback_args @ ..] = args else {
        return Err(RootedError::host("invalid diagnostics callback completion"));
    };
    let prefix = context
        .string_text(*prefix_root)?
        .ok_or_else(|| RootedError::host("invalid tracing channel prefix"))?;
    let error = callback_args.first().copied();
    if let Some(error) = error.filter(|error| context.truthy_rooted(*error).unwrap_or(false)) {
        set(context, *event_context, "error", error)?;
        publish_named(context, &trace_name(&prefix, "error"), *event_context)?;
    } else if let Some(result) = callback_args.get(1).copied() {
        set(context, *event_context, "result", result)?;
    }
    publish_named(context, &trace_name(&prefix, "asyncStart"), *event_context)?;
    publish_named(context, &trace_name(&prefix, "asyncEnd"), *event_context)?;
    context.call_rooted(*callback, *this_arg, callback_args)
}

/// HTTP owns timing and payload shape; it calls this after constructing the
/// Node request/response objects. Unobserved channels have no work to do.
pub(crate) fn publish_named(
    context: &mut NativeContext<'_, NodeHost>,
    name: &str,
    message: RootId,
) -> Result<(), RootedError> {
    let callbacks = {
        let state = context.host_mut().shared_state();
        let state = state.borrow();
        state
            .diagnostics
            .channels
            .get(name)
            .cloned()
            .unwrap_or_default()
    };
    let undefined = context.undefined();
    for callback in callbacks {
        let result = context.call_rooted(callback, undefined, &[message])?;
        context.release_root(result);
    }
    Ok(())
}

/// A snapshot array of the subscribers of channel `name`, or none when it has none: a host
/// publisher calls each with the message and the name, as `publish` does.
pub(crate) fn subscribers_rooted(
    runtime: &mut Runtime<NodeHost>,
    name: &str,
) -> Result<Option<RootId>, String> {
    let state = runtime.host_mut().shared_state();
    let callbacks = state.borrow().diagnostics.channels.get(name).filter(|callbacks| !callbacks.is_empty()).cloned();
    callbacks
        .map(|callbacks| runtime.array_rooted(&callbacks).map_err(|error| error.to_string()))
        .transpose()
}

fn callback_wrapper(
    context: &mut NativeContext<'_, NodeHost>,
    prefix: &str,
    event_context: RootId,
    callback: RootId,
) -> Result<RootId, RootedError> {
    let complete = context.host_function(crate::host::shared_vm::operation(
        "diagnosticsTraceComplete",
    ))?;
    let factory = crate::host::removed_javascript("node:diagnostics_channel/callback-wrapper.js")?;
    let prefix = context.string_rooted(prefix);
    let undefined = context.undefined();
    context.call_rooted(
        factory,
        undefined,
        &[complete, prefix, event_context, callback],
    )
}

fn subscribe_named(
    context: &mut NativeContext<'_, NodeHost>,
    name: &str,
    callback: RootId,
) -> Result<(), RootedError> {
    if !context.is_callable_rooted(callback)? {
        return Err(invalid_callback(context));
    }
    let subscribers = {
        let state = context.host_mut().shared_state();
        let state = state.borrow();
        state
            .diagnostics
            .channels
            .get(name)
            .cloned()
            .unwrap_or_default()
    };
    for current in subscribers {
        if context.same_value_rooted(current, callback)? {
            return Ok(());
        }
    }
    let callback = context.retain(callback)?;
    let state = context.host_mut().shared_state();
    state
        .borrow_mut()
        .diagnostics
        .channels
        .entry(name.to_owned())
        .or_default()
        .push(callback);
    Ok(())
}

fn has_tracing_subscribers(context: &mut NativeContext<'_, NodeHost>, prefix: &str) -> bool {
    let state = context.host_mut().shared_state();
    let state = state.borrow();
    TRACE_NAMES.iter().any(|event| {
        state
            .diagnostics
            .channels
            .get(&trace_name(prefix, event))
            .is_some_and(|subscribers| !subscribers.is_empty())
    })
}

fn callback_index(
    context: &mut NativeContext<'_, NodeHost>,
    position: Option<RootId>,
    argument_count: usize,
) -> Option<usize> {
    let index = position
        .and_then(|root| context.rooted_value(root))
        .and_then(|value| value.as_number())
        .filter(|value| value.is_finite() && value.fract() == 0.0)
        .map(|value| value as isize)
        .unwrap_or(0);
    let index = if index < 0 {
        argument_count as isize + index
    } else {
        index
    };
    usize::try_from(index)
        .ok()
        .filter(|index| *index < argument_count)
}

fn trace_prefix(
    context: &mut NativeContext<'_, NodeHost>,
    tracing: RootId,
) -> Result<String, RootedError> {
    let prefix = property(context, tracing, TRACE_PREFIX)?;
    context
        .string_text(prefix)?
        .ok_or_else(|| RootedError::host("invalid tracing channel object"))
}

fn trace_name(prefix: &str, event: &str) -> String {
    format!("tracing:{prefix}:{event}")
}

fn string_arg(
    context: &mut NativeContext<'_, NodeHost>,
    root: Option<RootId>,
    name: &str,
) -> Result<String, RootedError> {
    let Some(root) = root else {
        return Err(invalid_arg(context, name));
    };
    context
        .string_text(root)?
        .ok_or_else(|| invalid_arg(context, name))
}

fn is_undefined(context: &mut NativeContext<'_, NodeHost>, value: RootId) -> bool {
    context
        .rooted_value(value)
        .is_some_and(|value| value.is_undefined())
}

fn property(
    context: &mut NativeContext<'_, NodeHost>,
    object: RootId,
    name: &str,
) -> Result<RootId, RootedError> {
    let key = context.string_rooted(name);
    context.get_property_rooted(object, key)
}

fn install_operation(
    context: &mut NativeContext<'_, NodeHost>,
    object: RootId,
    property: &str,
    operation_name: &str,
) -> Result<(), RootedError> {
    let operation = crate::host::shared_vm::operation(operation_name);
    let function = context.host_function(operation)?;
    set(context, object, property, function)
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
            "cannot install diagnostics channel property {name}"
        )))
    }
}

fn invalid_callback(context: &mut NativeContext<'_, NodeHost>) -> RootedError {
    invalid_arg(context, "callback")
}

fn invalid_arg(context: &mut NativeContext<'_, NodeHost>, name: &str) -> RootedError {
    let error = context
        .type_error_rooted(&format!("The \"{name}\" argument must be of type function"))
        .expect("creating a TypeError cannot fail");
    let code = context.string_rooted("code");
    let value = context.string_rooted("ERR_INVALID_ARG_TYPE");
    let _ = context.set_property_rooted(error, code, value, error);
    context.throw(error)
}

crate::host::startup_snapshot::host_snap_struct!(SharedDiagnosticsState { channels, module });
