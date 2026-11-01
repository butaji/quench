//! Project the configured public crypto factory declarations and export
//! expressions from their original AST. Their implementation is never copied.
use crate::host::NodeHost;
use quench_runtime::{NativeContext, RootId, RootedError};
pub(crate) fn install(
    c: &mut NativeContext<'_, NodeHost>,
    public: RootId,
) -> Result<(), RootedError> {
    let _ = (c, public);
    crate::host::removed_javascript("node:crypto hash factories").map(drop)
}
