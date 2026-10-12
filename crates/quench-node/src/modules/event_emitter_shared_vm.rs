//! EventEmitter inspection derives copies and counts from its listener store.

use crate::host::NodeHost;
use quench_runtime::{NativeContext, RootId, RootedError};

const FACTORY: &str = quench_js_check::checked_js!(
    r#"(namesNative, listenersNative, rawNative, countNative) => ({
  eventNames: function eventNames() { 'use strict'; return namesNative(this); },
  listeners: function listeners(type) { 'use strict'; return listenersNative(this, type); },
  rawListeners: function rawListeners(type) { 'use strict'; return rawNative(this, type); },
  listenerCount: function listenerCount(type, listener) { 'use strict'; return countNative(this, type, listener); }
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
    let operations = [
        "emitterEventNames",
        "emitterListeners",
        "emitterRawListeners",
        "emitterListenerCount",
    ]
    .into_iter()
    .map(|name| context.host_function(crate::host::shared_vm::operation(name)))
    .collect::<Result<Vec<_>, _>>()?;
    let factory = context.evaluate_script_rooted(FACTORY, "node:events/inspection.js")?;
    let undefined = context.undefined();
    let api = context.call_rooted(factory, undefined, &operations)?;
    for name in ["eventNames", "listeners", "rawListeners", "listenerCount"] {
        let value = get(context, api, name)?;
        let key = context.string_rooted(name);
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
