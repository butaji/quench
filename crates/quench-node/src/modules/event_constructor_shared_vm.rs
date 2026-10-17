//! Callable EventEmitter construction and the canonical capture policy.

use crate::host::NodeHost;
use quench_runtime::{NativeContext, RootId, RootedError};

pub(super) fn get(
    context: &mut NativeContext<'_, NodeHost>,
    object: RootId,
    name: &str,
) -> Result<RootId, RootedError> {
    context.get_named_property_rooted(object, name)
}
/// Strict-mode `object[key] = value`, the assignment the event configuration's guest
/// `assign` operation performs.
fn nullish(context: &NativeContext<'_, NodeHost>, value: RootId) -> bool {
    context
        .rooted_value(value)
        .is_some_and(|value| value.is_null() || value.is_undefined())
}
/// The emitter configuration's members that `install` fixes: the private shape and capture
/// keys and the EventEmitter constructor, retained once instead of read per construction.
#[derive(Clone, Copy)]
pub(crate) struct EmitterKeys {
    shape: RootId,
    capture: RootId,
    constructor: RootId,
}

fn emitter_keys(
    context: &mut NativeContext<'_, NodeHost>,
    data: RootId,
) -> Result<EmitterKeys, RootedError> {
    let state = context.host_mut().shared_state();
    if let Some(keys) = state.borrow().emitter_keys {
        return Ok(keys);
    }
    let shape = get(context, data, "shapeKey")?;
    let capture = get(context, data, "captureKey")?;
    let constructor = get(context, data, "emitterCtor")?;
    let keys = EmitterKeys {
        shape: context.retain(shape)?,
        capture: context.retain(capture)?,
        constructor: context.retain(constructor)?,
    };
    state.borrow_mut().emitter_keys = Some(keys);
    Ok(keys)
}

fn capture_default(
    context: &mut NativeContext<'_, NodeHost>,
    data: RootId,
) -> Result<RootId, RootedError> {
    let keys = emitter_keys(context, data)?;
    let prototype = get(context, keys.constructor, "prototype")?;
    context.get_property_rooted(prototype, keys.capture)
}
fn validate_boolean(
    context: &mut NativeContext<'_, NodeHost>,
    value: RootId,
    name: &str,
) -> Result<(), RootedError> {
    if context
        .rooted_value(value)
        .is_some_and(|value| value.as_bool().is_some())
    {
        return Ok(());
    }
    Err(super::util_shared_vm::argument_type_error(
        context, value, name, "boolean",
    )?)
}

pub(crate) fn install(context: &mut NativeContext<'_, NodeHost>) -> Result<(), RootedError> {
    let data = context
        .host_mut()
        .shared_state()
        .borrow()
        .event_targets_api
        .ok_or_else(|| RootedError::host("emitter configuration missing"))?;
    let global = context.global_root()?;
    let constructor = get(context, global, "__nodeEventEmitter")?;
    context.assign_named_property_rooted(data, "emitterCtor", constructor)?;
    let prototype = get(context, constructor, "prototype")?;
    let zero = context.number(0.0);
    let undefined = context.undefined();
    for (name, value) in [
        ("_events", undefined),
        ("_eventsCount", zero),
        ("_maxListeners", undefined),
    ] {
        let key = context.string_rooted(name);
        context.define_data_property_rooted(prototype, key, value, true, true, true)?;
    }
    let key = get(context, data, "captureKey")?;
    let no = context.boolean(false);
    context.define_data_property_rooted(prototype, key, no, true, false, false)?;
    let get_native = context.host_function_with_data(
        crate::host::shared_vm::operation("emitterCaptureDefaultGet"),
        data,
    )?;
    let set_native = context.host_function_with_data(
        crate::host::shared_vm::operation("emitterCaptureDefaultSet"),
        data,
    )?;
    let factory = context.evaluate_script_rooted("(getNative, setNative) => ({get: function get() { return getNative(); }, set: function set(value) { return setNative(value); }})", "node:events/capture-default.js")?;
    let api = context.call_rooted(factory, undefined, &[get_native, set_native])?;
    let descriptor = context.null_object_rooted()?;
    for name in ["get", "set"] {
        let value = get(context, api, name)?;
        context.assign_named_property_rooted(descriptor, name, value)?;
    }
    let yes = context.boolean(true);
    context.assign_named_property_rooted(descriptor, "enumerable", yes)?;
    let key = context.string_rooted("captureRejections");
    context.define_property_descriptor_rooted(constructor, key, descriptor)?;
    Ok(())
}

pub(crate) fn initialize(
    context: &mut NativeContext<'_, NodeHost>,
    _: RootId,
    args: &[RootId],
) -> Result<RootId, RootedError> {
    let undefined = context.undefined();
    let target = args.first().copied().unwrap_or(undefined);
    let options = args.get(1).copied().unwrap_or(undefined);
    let data = context.host_function_data()?;
    let first = get(context, target, "_events")?;
    let initialize = if context
        .rooted_value(first)
        .is_some_and(|value| value.is_undefined())
    {
        true
    } else {
        let own = get(context, target, "_events")?;
        let prototype = context.prototype_rooted(target)?;
        let inherited = get(context, prototype, "_events")?;
        context.strict_equal_rooted(own, inherited)?
    };
    if initialize {
        let events = context.null_object_rooted()?;
        context.assign_named_property_rooted(target, "_events", events)?;
        let zero = context.number(0.0);
        context.assign_named_property_rooted(target, "_eventsCount", zero)?;
    }
    let shape = context.boolean(!initialize);
    let keys = emitter_keys(context, data)?;
    context.assign_property_rooted(target, keys.shape, shape)?;
    let maximum = get(context, target, "_maxListeners")?;
    if !context.truthy_rooted(maximum)? {
        context.assign_named_property_rooted(target, "_maxListeners", undefined)?;
    }
    let first_capture = if nullish(context, options) {
        undefined
    } else {
        get(context, options, "captureRejections")?
    };
    let capture = if context.truthy_rooted(first_capture)? {
        let value = get(context, options, "captureRejections")?;
        validate_boolean(context, value, "options.captureRejections")?;
        let value = get(context, options, "captureRejections")?;
        let enabled = context.truthy_rooted(value)?;
        context.boolean(enabled)
    } else {
        capture_default(context, data)?
    };
    context.assign_property_rooted(target, keys.capture, capture)?;
    // Implicit domain association does not enroll an emitter in explicit members.
    let global = context.global_root()?;
    let domain = get(context, global, "__quench_active_domain")?;
    if context.truthy_rooted(domain)? {
        context.assign_named_property_rooted(target, "domain", domain)?;
    }
    Ok(undefined)
}

pub(crate) fn default_get(
    context: &mut NativeContext<'_, NodeHost>,
    _: RootId,
    _: &[RootId],
) -> Result<RootId, RootedError> {
    let data = context.host_function_data()?;
    capture_default(context, data)
}
pub(crate) fn default_set(
    context: &mut NativeContext<'_, NodeHost>,
    _: RootId,
    args: &[RootId],
) -> Result<RootId, RootedError> {
    let value = args.first().copied().unwrap_or_else(|| context.undefined());
    validate_boolean(context, value, "EventEmitter.captureRejections")?;
    let data = context.host_function_data()?;
    let constructor = get(context, data, "emitterCtor")?;
    let prototype = get(context, constructor, "prototype")?;
    let key = get(context, data, "captureKey")?;
    context.assign_property_rooted(prototype, key, value)?;
    Ok(context.undefined())
}

/// Metadata transitions follow creation/removal of the event's canonical record.
pub(crate) fn event_added(
    context: &mut NativeContext<'_, NodeHost>,
    target: RootId,
) -> Result<(), RootedError> {
    let count = get(context, target, "_eventsCount")?;
    let next = context.numeric_update_rooted(count, false)?;
    context.assign_named_property_rooted(target, "_eventsCount", next)
}
pub(super) fn event_removed(
    context: &mut NativeContext<'_, NodeHost>,
    data: RootId,
    target: RootId,
    events: RootId,
    event: RootId,
) -> Result<(), RootedError> {
    let count = get(context, target, "_eventsCount")?;
    let one = context.number(1.0);
    let next = context.subtract_rooted(count, one)?;
    context.assign_named_property_rooted(target, "_eventsCount", next)?;
    let key = get(context, data, "shapeKey")?;
    let shape = context.get_property_rooted(target, key)?;
    let zero = context.number(0.0);
    if context.truthy_rooted(shape)? {
        let undefined = context.undefined();
        context.assign_property_rooted(events, event, undefined)?;
    } else if context.strict_equal_rooted(next, zero)? {
        let events = context.null_object_rooted()?;
        context.assign_named_property_rooted(target, "_events", events)?;
    } else {
        let delete = get(context, data, "delete")?;
        let undefined = context.undefined();
        context.call_rooted(delete, undefined, &[events, event])?;
    }
    Ok(())
}

/// Node observes thenables only while the emitter's private capture flag is set.
pub(super) fn capture_result(
    context: &mut NativeContext<'_, NodeHost>,
    data: RootId,
    target: RootId,
    result: RootId,
    event: RootId,
    event_args: RootId,
) -> Result<RootId, RootedError> {
    let undefined = context.undefined();
    if nullish(context, result) {
        return Ok(undefined);
    }
    let key = get(context, data, "captureKey")?;
    let enabled = context.get_property_rooted(target, key)?;
    if !context.truthy_rooted(enabled)? {
        return Ok(undefined);
    }
    let captured: Result<RootId, RootedError> = (|| {
        let then = get(context, result, "then")?;
        if context.is_callable_rooted(then)? {
            let callback_data = context.array_rooted(&[data, target, event, event_args])?;
            let rejected = context.host_function_with_data(
                crate::host::shared_vm::operation("emitterCaptureRejected"),
                callback_data,
            )?;
            context.call_rooted(then, result, &[undefined, rejected])?;
        }
        Ok(undefined)
    })();
    match captured {
        Ok(value) => Ok(value),
        Err(error) => {
            let exception = if let Some(exception) = error.exception {
                exception
            } else {
                context.error_rooted(&error.to_string())?
            };
            let emit = get(context, target, "emit")?;
            let event = context.string_rooted("error");
            context.call_rooted(emit, target, &[event, exception])
        }
    }
}
pub(crate) fn capture_rejected(
    context: &mut NativeContext<'_, NodeHost>,
    _: RootId,
    args: &[RootId],
) -> Result<RootId, RootedError> {
    let data = context.host_function_data()?;
    let callback = context.host_function_with_data(
        crate::host::shared_vm::operation("emitterCaptureDeliver"),
        data,
    )?;
    let undefined = context.undefined();
    let error = args.first().copied().unwrap_or(undefined);
    super::shared_event_loop::queue_next_tick(context, callback, undefined, &[error])?;
    Ok(undefined)
}
pub(crate) fn capture_deliver(
    context: &mut NativeContext<'_, NodeHost>,
    _: RootId,
    args: &[RootId],
) -> Result<RootId, RootedError> {
    let undefined = context.undefined();
    let callback_data = context.host_function_data()?;
    let data = get(context, callback_data, "0")?;
    let target = get(context, callback_data, "1")?;
    let event = get(context, callback_data, "2")?;
    let event_args = get(context, callback_data, "3")?;
    let error = args.first().copied().unwrap_or(undefined);
    let rejection_key = get(context, data, "rejectionKey")?;
    let rejection = context.get_property_rooted(target, rejection_key)?;
    if context.is_callable_rooted(rejection)? {
        let rejection = context.get_property_rooted(target, rejection_key)?;
        let length = get(context, event_args, "length")?;
        let length = context
            .rooted_value(length)
            .and_then(|value| value.as_number())
            .unwrap_or_default() as usize;
        let mut arguments = vec![error, event];
        for index in 0..length {
            arguments.push(get(context, event_args, &index.to_string())?);
        }
        return context.call_rooted(rejection, target, &arguments);
    }
    let key = get(context, data, "captureKey")?;
    let previous = context.get_property_rooted(target, key)?;
    let no = context.boolean(false);
    context.assign_property_rooted(target, key, no)?;
    let result: Result<RootId, RootedError> = (|| {
        let emit = get(context, target, "emit")?;
        let error_event = context.string_rooted("error");
        context.call_rooted(emit, target, &[error_event, error])
    })();
    if let Err(cleanup) = context.assign_property_rooted(target, key, previous) {
        if let Err(error) = result {
            if let Some(exception) = error.exception {
                context.release_root(exception);
            }
        }
        return Err(cleanup);
    }
    result
}
