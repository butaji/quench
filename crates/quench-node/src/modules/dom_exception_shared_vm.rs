//! Original SDK DOM exception classes and their one source-owned identity.
use crate::host::NodeHost;
use quench_runtime::{NativeContext, RootId, RootedError};
type Context<'a> = NativeContext<'a, NodeHost>;
pub(crate) fn module(c: &mut Context<'_>) -> Result<RootId, RootedError> {
    crate::host::shared_vm::commonjs::node_library_module(
        c,
        "internal/util.js",
        "internal/per_context/domexception.js",
    )
}
pub(crate) fn initialize(
    c: &mut Context<'_>,
    _: RootId,
    _: &[RootId],
) -> Result<RootId, RootedError> {
    let module = module(c)?;
    let global = c.global_root()?;
    for name in ["DOMException", "QuotaExceededError"] {
        let key = c.string_rooted(name);
        let value = c.get_property_rooted(module, key)?;
        c.define_data_property_rooted(global, key, value, true, false, true)?;
    }
    Ok(c.undefined())
}
