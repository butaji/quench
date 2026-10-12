//! Node's shared listener limit and its emitter/target projections.

use crate::host::NodeHost;
use quench_runtime::{NativeContext, RootId, RootedError};

pub(crate) const DEFAULT_MAX_LISTENERS: f64 = 10.0;
// AbortSignal disables leak warnings by default, independently of emitter defaults.
pub(crate) const ABORT_SIGNAL_MAX_LISTENERS: f64 = 0.0;

const FACTORY: &str = quench_js_check::checked_js!(
    r#"(defaultGetNative, defaultSetNative, getNative, setNative, globalGetNative, globalSetNative) => ({
  defaultGet: function get() { return defaultGetNative(); },
  defaultSet: function set(value) { return defaultSetNative(value); },
  get: function getMaxListeners() { 'use strict'; return getNative(this); },
  set: function setMaxListeners(value) { 'use strict'; return setNative(this, value); },
  globalGet: function getMaxListeners(target) { return globalGetNative(target); },
  globalSet: function setMaxListeners(value = undefined, ...targets) { return globalSetNative(value, ...targets); }
})"#
);

fn get(
    context: &mut NativeContext<'_, NodeHost>,
    object: RootId,
    name: &str,
) -> Result<RootId, RootedError> {
    let key = context.string_rooted(name);
    context.get_property_rooted(object, key)
}
fn set(
    context: &mut NativeContext<'_, NodeHost>,
    object: RootId,
    name: &str,
    value: RootId,
) -> Result<(), RootedError> {
    let key = context.string_rooted(name);
    context.set_property_rooted(object, key, value, object)?;
    Ok(())
}
// Keep strict JavaScript assignment errors in the runtime's property semantics.
fn assign(
    context: &mut NativeContext<'_, NodeHost>,
    data: RootId,
    object: RootId,
    key: RootId,
    value: RootId,
) -> Result<(), RootedError> {
    let operation = get(context, data, "assign")?;
    let undefined = context.undefined();
    context.call_rooted(operation, undefined, &[object, key, value])?;
    Ok(())
}
fn nullish(context: &NativeContext<'_, NodeHost>, value: RootId) -> bool {
    context
        .rooted_value(value)
        .is_some_and(|value| value.is_null() || value.is_undefined())
}
fn validate(
    context: &mut NativeContext<'_, NodeHost>,
    value: RootId,
    name: &str,
) -> Result<(), RootedError> {
    let Some(number) = context
        .rooted_value(value)
        .and_then(|value| value.as_number())
    else {
        return Err(super::util_shared_vm::argument_type_error(
            context, value, name, "number",
        )?);
    };
    if number.is_nan() || number < 0.0 {
        let received = super::util_shared_vm::range_error_received(context, value)?;
        let error = context.range_error_rooted(&format!(
            "The value of \"{name}\" is out of range. It must be >= 0. Received {received}"
        ))?;
        let code = context.string_rooted("ERR_OUT_OF_RANGE");
        set(context, error, "code", code)?;
        return Err(context.throw(error));
    }
    Ok(())
}

pub(crate) fn initialize(context: &mut NativeContext<'_, NodeHost>) -> Result<(), RootedError> {
    let state = context.host_mut().shared_state();
    let data = state
        .borrow()
        .event_targets_api
        .ok_or_else(|| RootedError::host("event listener configuration missing"))?;
    let mut operations = Vec::new();
    for name in [
        "eventsDefaultMaxGet",
        "eventsDefaultMaxSet",
        "emitterMaxGet",
        "emitterMaxSet",
        "eventsMaxGet",
        "eventsMaxSet",
    ] {
        operations
            .push(context.host_function_with_data(crate::host::shared_vm::operation(name), data)?);
    }
    let factory = context.evaluate_script_rooted(FACTORY, "node:events/limits.js")?;
    let undefined = context.undefined();
    let api = context.call_rooted(factory, undefined, &operations)?;
    let global = context.global_root()?;
    let emitter = get(context, global, "__nodeEventEmitter")?;
    let prototype = get(context, emitter, "prototype")?;
    for (target, name, projection) in [
        (prototype, "getMaxListeners", "get"),
        (prototype, "setMaxListeners", "set"),
        (emitter, "getMaxListeners", "globalGet"),
        (emitter, "setMaxListeners", "globalSet"),
    ] {
        let key = context.string_rooted(name);
        let value = get(context, api, projection)?;
        context.define_data_property_rooted(target, key, value, true, true, true)?;
    }
    let descriptor = context.null_object_rooted()?;
    let getter = get(context, api, "defaultGet")?;
    let setter = get(context, api, "defaultSet")?;
    set(context, descriptor, "get", getter)?;
    set(context, descriptor, "set", setter)?;
    let yes = context.boolean(true);
    set(context, descriptor, "enumerable", yes)?;
    let key = context.string_rooted("defaultMaxListeners");
    context.define_property_descriptor_rooted(emitter, key, descriptor)?;
    for (name, field) in [
        ("kMaxEventTargetListeners", "maxKey"),
        ("kMaxEventTargetListenersWarned", "warnedKey"),
    ] {
        let key = context.string_rooted(name);
        let value = get(context, data, field)?;
        context.define_data_property_rooted(emitter, key, value, false, false, false)?;
    }
    Ok(())
}

pub(crate) fn default_get(
    context: &mut NativeContext<'_, NodeHost>,
    _: RootId,
    _: &[RootId],
) -> Result<RootId, RootedError> {
    let data = context.host_function_data()?;
    get(context, data, "defaultMaxListeners")
}
pub(crate) fn default_set(
    context: &mut NativeContext<'_, NodeHost>,
    _: RootId,
    args: &[RootId],
) -> Result<RootId, RootedError> {
    let undefined = context.undefined();
    let value = args.first().copied().unwrap_or(undefined);
    validate(context, value, "defaultMaxListeners")?;
    let data = context.host_function_data()?;
    set(context, data, "defaultMaxListeners", value)?;
    Ok(undefined)
}
fn emitter_max(
    context: &mut NativeContext<'_, NodeHost>,
    data: RootId,
    emitter: RootId,
) -> Result<RootId, RootedError> {
    let value = get(context, emitter, "_maxListeners")?;
    if context
        .rooted_value(value)
        .is_some_and(|value| value.is_undefined())
    {
        get(context, data, "defaultMaxListeners")
    } else {
        Ok(value)
    }
}
pub(crate) fn emitter_get(
    context: &mut NativeContext<'_, NodeHost>,
    _: RootId,
    args: &[RootId],
) -> Result<RootId, RootedError> {
    let undefined = context.undefined();
    let target = args.first().copied().unwrap_or(undefined);
    let data = context.host_function_data()?;
    emitter_max(context, data, target)
}
pub(crate) fn emitter_set(
    context: &mut NativeContext<'_, NodeHost>,
    _: RootId,
    args: &[RootId],
) -> Result<RootId, RootedError> {
    let undefined = context.undefined();
    let target = args.first().copied().unwrap_or(undefined);
    let value = args.get(1).copied().unwrap_or(undefined);
    validate(context, value, "setMaxListeners")?;
    let data = context.host_function_data()?;
    let key = context.string_rooted("_maxListeners");
    assign(context, data, target, key, value)?;
    Ok(target)
}
pub(crate) fn global_get(
    context: &mut NativeContext<'_, NodeHost>,
    _: RootId,
    args: &[RootId],
) -> Result<RootId, RootedError> {
    let undefined = context.undefined();
    let target = args.first().copied().unwrap_or(undefined);
    let data = context.host_function_data()?;
    if !nullish(context, target) {
        let method = get(context, target, "getMaxListeners")?;
        if context.is_callable_rooted(method)? {
            return emitter_max(context, data, target);
        }
        let key = get(context, data, "maxKey")?;
        let maximum = context.get_property_rooted(target, key)?;
        if context
            .rooted_value(maximum)
            .is_some_and(|value| value.as_number().is_some())
        {
            return Ok(maximum);
        }
    }
    Err(super::util_shared_vm::argument_type_error(
        context,
        target,
        "emitterOrTarget",
        "EventEmitter or EventTarget",
    )?)
}
pub(crate) fn global_set(
    context: &mut NativeContext<'_, NodeHost>,
    _: RootId,
    args: &[RootId],
) -> Result<RootId, RootedError> {
    let undefined = context.undefined();
    let data = context.host_function_data()?;
    let mut maximum = args.first().copied().unwrap_or(undefined);
    if context
        .rooted_value(maximum)
        .is_some_and(|value| value.is_undefined())
    {
        maximum = get(context, data, "defaultMaxListeners")?;
    }
    validate(context, maximum, "setMaxListeners")?;
    if args.len() <= 1 {
        set(context, data, "defaultMaxListeners", maximum)?;
        return Ok(undefined);
    }
    for target in args[1..].iter().copied() {
        let constructor = if nullish(context, target) {
            undefined
        } else {
            get(context, target, "constructor")?
        };
        let branded = if nullish(context, constructor) {
            false
        } else {
            let key = get(context, data, "brandKey")?;
            let brand = context.get_property_rooted(constructor, key)?;
            context.truthy_rooted(brand)?
        };
        if branded {
            let key = get(context, data, "maxKey")?;
            assign(context, data, target, key, maximum)?;
            let key = get(context, data, "warnedKey")?;
            let no = context.boolean(false);
            assign(context, data, target, key, no)?;
        } else {
            let method = get(context, target, "setMaxListeners")?;
            if !context.is_callable_rooted(method)? {
                return Err(super::util_shared_vm::argument_type_error(
                    context,
                    target,
                    "eventTargets",
                    "EventEmitter or EventTarget",
                )?);
            }
            let method = get(context, target, "setMaxListeners")?;
            context.call_rooted(method, target, &[maximum])?;
        }
    }
    Ok(undefined)
}

/// Derive warning metadata after the bridge installs the new listener array.
/// Warning state belongs to the canonical listener array.
pub(super) fn registered(
    context: &mut NativeContext<'_, NodeHost>,
    data: RootId,
    target: RootId,
    event: RootId,
    previous: RootId,
    current: RootId,
) -> Result<RootId, RootedError> {
    let undefined = context.undefined();
    if context
        .rooted_value(previous)
        .is_some_and(|value| value.is_undefined())
    {
        super::event_constructor_shared_vm::event_added(context, data, target)?;
    }
    if !context.is_array_rooted(current)? {
        return Ok(undefined);
    }
    let limit = emitter_max(context, data, target)?;
    let limit_value = context
        .rooted_value(limit)
        .and_then(|value| value.as_number())
        .unwrap_or_default();
    let count = get(context, current, "length")?;
    let count_value = context
        .rooted_value(count)
        .and_then(|value| value.as_number())
        .unwrap_or_default();
    if !(limit_value > 0.0 && count_value > limit_value) {
        return Ok(undefined);
    }
    let warned = get(context, current, "warned")?;
    if context.truthy_rooted(warned)? {
        return Ok(undefined);
    }
    let yes = context.boolean(true);
    set(context, current, "warned", yes)?;
    let utility = super::util_shared_vm::module(context)?;
    let inspect = super::util_shared_vm::internal_inspect(context)?;
    let options = context.null_object_rooted()?;
    let depth = context.number(-1.0);
    set(context, options, "depth", depth)?;
    let target_text = context.call_rooted(inspect, utility, &[target, options])?;
    let target_text = context.to_string(target_text)?;
    let string = get(context, data, "stringCtor")?;
    let event_text = context.call_rooted(string, undefined, &[event])?;
    let event_text = context.to_string(event_text)?;
    let limit_text = context.to_string(limit)?;
    let count_text = context.to_string(count)?;
    let warning = context.error_rooted(&format!(
        "Possible EventEmitter memory leak detected. {count_text} {event_text} listeners added to {target_text}. MaxListeners is {limit_text}. Use emitter.setMaxListeners() to increase limit"))?;
    let name = context.string_rooted("MaxListenersExceededWarning");
    set(context, warning, "name", name)?;
    set(context, warning, "emitter", target)?;
    set(context, warning, "type", event)?;
    set(context, warning, "count", count)?;
    let global = context.global_root()?;
    let process = get(context, global, "process")?;
    let emit_warning = get(context, process, "emitWarning")?;
    context.call_rooted(emit_warning, process, &[warning])?;
    Ok(undefined)
}
