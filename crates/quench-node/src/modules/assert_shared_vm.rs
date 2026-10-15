//! Canonical configured Node assertion and comparison policies.
use crate::host::NodeHost;
use quench_runtime::{NativeContext, RootId, RootedError};

pub(crate) fn module(
    context: &mut NativeContext<'_, NodeHost>,
    _: RootId,
) -> Result<RootId, RootedError> {
    crate::host::shared_vm::commonjs::node_library_module(context, "assert.js", "assert.js")
}

/// util and assert use the same comparison owner; load it after util initializes.
pub(crate) fn is_deep_strict_equal(
    context: &mut NativeContext<'_, NodeHost>,
    _: RootId,
    args: &[RootId],
) -> Result<RootId, RootedError> {
    let compare = crate::host::shared_vm::commonjs::node_library_export(
        context,
        "util.js",
        "internal/util/comparisons.js",
        "isDeepStrictEqual",
    )?;
    let missing = context.undefined();
    context.call_rooted(compare, missing, args)
}
