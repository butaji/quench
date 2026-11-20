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
