//! Shared `perf_hooks` projection for APIs used by Node packages.

use crate::host::NodeHost;
use quench_runtime::{NativeContext, RootId, RootedError};

pub(crate) fn module(context: &mut NativeContext<'_, NodeHost>) -> Result<RootId, RootedError> {
    let module = context.object_rooted()?;
    let performance = context.object_rooted()?;
    let now = context.host_function(crate::host::shared_vm::operation("perfHooksNow"))?;
    set(context, performance, "now", now)?;
    set(context, module, "performance", performance)?;
    Ok(module)
}

pub(crate) fn now(
    context: &mut NativeContext<'_, NodeHost>,
    _: RootId,
    _: &[RootId],
) -> Result<RootId, RootedError> {
    let elapsed_ms = context
        .host_mut()
        .shared_state()
        .borrow()
        .process_control
        .uptime()
        * 1_000.0;
    Ok(context.number(elapsed_ms))
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
            "cannot install perf_hooks.{name}"
        )))
    }
}
