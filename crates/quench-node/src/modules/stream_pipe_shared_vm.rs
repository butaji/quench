//! Pipe completion keeps legacy close destruction separate from Readable EOF.
use crate::host::NodeHost;
use quench_runtime::{NativeContext, RootId, RootedError};

const DESTINATION: &str = "destination";
const LEGACY: &str = "legacy";
const COMPLETED: &str = "completed";

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

pub(crate) fn plan(
    context: &mut NativeContext<'_, NodeHost>,
    _: RootId,
    args: &[RootId],
) -> Result<RootId, RootedError> {
    let source = args
        .first()
        .copied()
        .ok_or_else(|| RootedError::host("pipe source missing"))?;
    let destination = args
        .get(1)
        .copied()
        .ok_or_else(|| RootedError::host("pipe destination missing"))?;
    let undefined = context.undefined();
    let options = args.get(2).copied().unwrap_or(undefined);
    let readable_state = get(context, source, "_readableState")?;
    let legacy = if context.truthy_rooted(readable_state)? {
        let flag = get(context, readable_state, "streamBase")?;
        context.truthy_rooted(flag)?
    } else {
        false
    };
    let should_end = if legacy {
        let stdio = get(context, destination, "_isStdio")?;
        if context.truthy_rooted(stdio)? {
            false
        } else if context.truthy_rooted(options)? {
            let end = get(context, options, "end")?;
            let false_value = context.boolean(false);
            !context.strict_equal_rooted(end, false_value)?
        } else {
            true
        }
    } else {
        let end =
            super::stream_shared_vm::pipe_should_end(context, undefined, &[destination, options])?;
        context.truthy_rooted(end)?
    };
    let data = context.null_object_rooted()?;
    set(context, data, DESTINATION, destination)?;
    let legacy_value = context.boolean(legacy);
    set(context, data, LEGACY, legacy_value)?;
    let pending = context.boolean(false);
    set(context, data, COMPLETED, pending)?;
    let result = context.null_object_rooted()?;
    for (name, enabled) in [
        ("onceEnd", !legacy),
        ("cleanupOnEnd", legacy),
        ("cleanupOnClose", legacy),
    ] {
        let enabled = context.boolean(enabled);
        set(context, result, name, enabled)?;
    }
    for (name, operation, enabled) in [
        ("end", "streamPipeEnd", should_end),
        ("close", "streamPipeClose", should_end && legacy),
    ] {
        let callback = if enabled {
            context.host_function_with_data(crate::host::shared_vm::operation(operation), data)?
        } else {
            context.null()
        };
        set(context, result, name, callback)?;
    }
    Ok(result)
}

/// Complete a legacy pipe before calling its destination, preserving identity
/// and the once-only transition if a guest end/destroy hook throws.
fn begin_completion(
    context: &mut NativeContext<'_, NodeHost>,
) -> Result<Option<RootId>, RootedError> {
    let data = context.host_function_data()?;
    let legacy = get(context, data, LEGACY)?;
    if context.truthy_rooted(legacy)? {
        let completed = get(context, data, COMPLETED)?;
        if context.truthy_rooted(completed)? {
            return Ok(None);
        }
        let completed = context.boolean(true);
        set(context, data, COMPLETED, completed)?;
    }
    Ok(Some(get(context, data, DESTINATION)?))
}

pub(crate) fn end(
    context: &mut NativeContext<'_, NodeHost>,
    _: RootId,
    _: &[RootId],
) -> Result<RootId, RootedError> {
    let Some(destination) = begin_completion(context)? else {
        return Ok(context.undefined());
    };
    let end = get(context, destination, "end")?;
    context.call_rooted(end, destination, &[])?;
    Ok(context.undefined())
}
pub(crate) fn close(
    context: &mut NativeContext<'_, NodeHost>,
    _: RootId,
    _: &[RootId],
) -> Result<RootId, RootedError> {
    let Some(destination) = begin_completion(context)? else {
        return Ok(context.undefined());
    };
    let destroy = get(context, destination, "destroy")?;
    if context.is_callable_rooted(destroy)? {
        context.call_rooted(destroy, destination, &[])?;
    }
    Ok(context.undefined())
}


/// Pipe actively requests flow even when data listeners respect explicit pause.
pub(crate) fn start(context: &mut NativeContext<'_, NodeHost>, _: RootId, args: &[RootId]) -> Result<RootId, RootedError> {
    let source = args[0];
    let drain = get(context, args[1], "writableNeedDrain")?;
    let yes = context.boolean(true);
    let operation = if context.strict_equal_rooted(drain, yes)? { Some("pause") } else {
        let state = get(context, source, "_readableState")?;
        let flow = get(context, state, "flowing")?;
        if context.truthy_rooted(flow)? { None } else { Some("resume") }
    };
    if let Some(operation) = operation {
        let method = get(context, source, operation)?;
        context.call_rooted(method, source, &[])?;
    }
    Ok(context.undefined())
}
