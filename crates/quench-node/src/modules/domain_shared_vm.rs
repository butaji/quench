//! The configured SDK owns Domain, hook pairing, capture, and event policy.
use crate::host::NodeHost;
use quench_runtime::{NativeContext, RootId, RootedError};
pub(crate) fn module(c: &mut NativeContext<'_, NodeHost>) -> Result<RootId, RootedError> {
    crate::host::shared_vm::commonjs::node_library_module(c, "domain.js", "domain.js")
}
