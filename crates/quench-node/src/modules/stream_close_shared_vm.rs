//! Native close emission policy for the existing guest stream lifecycle.
use super::event_constructor_shared_vm::get;
use crate::host::NodeHost;
use quench_runtime::{NativeContext, RootId, RootedError};

pub(crate) fn option(
    context: &mut NativeContext<'_, NodeHost>,
    _: RootId,
    args: &[RootId],
) -> Result<RootId, RootedError> {
    let options = args[0];
    let value = get(context, options, "emitClose")?;
    let no = context.boolean(false);
    let enabled = !context.strict_equal_rooted(value, no)?;
    Ok(context.boolean(enabled))
}

pub(crate) fn emit(
    context: &mut NativeContext<'_, NodeHost>,
    _: RootId,
    args: &[RootId],
) -> Result<RootId, RootedError> {
    let stream = args[0];
    let mut enabled = false;
    for name in ["_readableState", "_writableState"] {
        let state = get(context, stream, name)?;
        if context.truthy_rooted(state)? {
            let flag = get(context, state, "emitClose")?;
            enabled |= context.truthy_rooted(flag)?;
            let key = context.string_rooted("closeEmitted");
            let yes = context.boolean(true);
            context.set_property_rooted(state, key, yes, state)?;
        }
    }
    let closed = context.string_rooted("closed");
    let yes = context.boolean(true);
    context.set_property_rooted(stream, closed, yes, stream)?;
    if enabled {
        let emitter = get(context, stream, "_emitter")?;
        let emit = get(context, emitter, "emit")?;
        let event = context.string_rooted("close");
        context.call_rooted(emit, emitter, &[event])?;
    }
    Ok(context.undefined())
}
