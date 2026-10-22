//! Genuine OS memory facts for process APIs, independent of guest fs overrides.
use crate::host::NodeHost;
use quench_runtime::{NativeContext, RootId, RootedError};
pub(crate) fn available(
    context: &mut NativeContext<'_, NodeHost>,
    _: RootId,
    _: &[RootId],
) -> Result<RootId, RootedError> {
    Ok(context.number(super::memory_facts::available() as f64))
}
pub(crate) fn constrained(
    context: &mut NativeContext<'_, NodeHost>,
    _: RootId,
    _: &[RootId],
) -> Result<RootId, RootedError> {
    Ok(context.number(super::memory_facts::constrained() as f64))
}
pub(crate) fn install(context: &mut NativeContext<'_, NodeHost>) -> Result<(), RootedError> {
    let process = context
        .host_mut()
        .shared_state()
        .borrow()
        .process_module
        .ok_or_else(|| RootedError::host("process memory surface has no process owner"))?;
    for name in ["availableMemory", "constrainedMemory"] {
        let key = context.string_rooted(name);
        let existing = context.get_property_rooted(process, key)?;
        if context.truthy_rooted(existing)? {
            continue;
        }
        let function = context.host_function(crate::host::shared_vm::operation(name))?;
        context.define_data_property_rooted(process, key, function, true, true, true)?;
    }
    Ok(())
}
