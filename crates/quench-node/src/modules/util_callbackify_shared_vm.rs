//! Native callbackify lifecycle: call original, settle, queue, invoke.
use crate::host::NodeHost;
use quench_runtime::{NativeContext, RootId, RootedError};

pub(crate) fn function(context: &mut NativeContext<'_, NodeHost>) -> Result<RootId, RootedError> {
    // Thin ordinary-function bridge preserves construction and `this`.
    let factory = context.evaluate_script_rooted(
        "\"use strict\"; (invoke) => function callbackified(...args) { return invoke(this, args); }",
        "node:util/callbackified.js",
    )?;
    context.host_function_with_data(crate::host::shared_vm::operation("callbackify"), factory)
}

pub(crate) fn callbackify(
    context: &mut NativeContext<'_, NodeHost>,
    _: RootId,
    args: &[RootId],
) -> Result<RootId, RootedError> {
    let original = args.first().copied().unwrap_or_else(|| context.undefined());
    if !context.is_callable_rooted(original)? {
        return Err(super::util_shared_vm::argument_type_error(
            context, original, "original", "function",
        )?);
    }
    let factory = context.host_function_data()?;
    let invoke = context.host_function_with_data(
        crate::host::shared_vm::operation("utilCallbackified"),
        original,
    )?;
    let undefined = context.undefined();
    let function = context.call_rooted(factory, undefined, &[invoke])?;
    let null = context.null();
    for key in context.own_keys_rooted(original)? {
        let descriptor = context.own_property_descriptor_rooted(original, key)?;
        context.set_prototype_rooted(descriptor, null)?;
        let name = context.string_text(key)?;
        if matches!(name.as_deref(), Some("name" | "length")) {
            let value = get(context, descriptor, "value")?;
            let updated = match name.as_deref() {
                Some("name") => context
                    .string_text(value)?
                    .map(|name| context.string_rooted(&format!("{name}Callbackified"))),
                _ => context
                    .rooted_value(value)
                    .and_then(|value| value.as_number())
                    .map(|length| context.number(length + 1.0)),
            };
            if let Some(updated) = updated {
                set(context, descriptor, "value", updated)?;
            }
        }
        context.define_property_descriptor_rooted(function, key, descriptor)?;
    }
    Ok(function)
}

pub(crate) fn callbackified(
    context: &mut NativeContext<'_, NodeHost>,
    _: RootId,
    args: &[RootId],
) -> Result<RootId, RootedError> {
    let original = context.host_function_data()?;
    let receiver = args[0];
    let arguments = args[1];
    let length = get(context, arguments, "length")?;
    let length = context
        .rooted_value(length)
        .and_then(|value| value.as_number())
        .unwrap_or(0.0) as usize;
    let callback = if length == 0 {
        context.undefined()
    } else {
        get(context, arguments, &(length - 1).to_string())?
    };
    if !context.is_callable_rooted(callback)? {
        return Err(super::util_shared_vm::argument_type_error(
            context,
            callback,
            "last argument",
            "function",
        )?);
    }
    let original_args = (0..length.saturating_sub(1))
        .map(|index| get(context, arguments, &index.to_string()))
        .collect::<Result<Vec<_>, _>>()?;
    let promise = context.call_rooted(original, receiver, &original_args)?;
    let then = get(context, promise, "then")?;
    let mut handlers = Vec::with_capacity(2);
    for rejection in [false, true] {
        let rejection = context.boolean(rejection);
        let data = context.array_rooted(&[callback, receiver, rejection])?;
        handlers.push(context.host_function_with_data(
            crate::host::shared_vm::operation("utilCallbackifySettled"),
            data,
        )?);
    }
    context.call_rooted(then, promise, &handlers)?;
    Ok(context.undefined())
}

pub(crate) fn settled(
    context: &mut NativeContext<'_, NodeHost>,
    _: RootId,
    args: &[RootId],
) -> Result<RootId, RootedError> {
    let data = context.host_function_data()?;
    let forward = context.host_function_with_data(
        crate::host::shared_vm::operation("utilCallbackifyInvoke"),
        data,
    )?;
    let value = args.first().copied().unwrap_or_else(|| context.undefined());
    let undefined = context.undefined();
    super::process_shared_vm::next_tick(context, undefined, &[forward, value])
}

pub(crate) fn invoke(
    context: &mut NativeContext<'_, NodeHost>,
    _: RootId,
    args: &[RootId],
) -> Result<RootId, RootedError> {
    let data = context.host_function_data()?;
    let callback = get(context, data, "0")?;
    let receiver = get(context, data, "1")?;
    let rejection = get(context, data, "2")?;
    let value = args.first().copied().unwrap_or_else(|| context.undefined());
    if context.truthy_rooted(rejection)? {
        let error = if context.truthy_rooted(value)? {
            value
        } else {
            let error = context.error_rooted("Promise was rejected with falsy value")?;
            let code = context.string_rooted("ERR_FALSY_VALUE_REJECTION");
            set(context, error, "code", code)?;
            set(context, error, "reason", value)?;
            error
        };
        context.call_rooted(callback, receiver, &[error])
    } else {
        let null = context.null();
        context.call_rooted(callback, receiver, &[null, value])
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
