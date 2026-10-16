//! Original Node Buffer validators and coded constructors, captured before guests.
use crate::host::NodeHost;
use quench_runtime::{NativeContext, RootId, RootedError};
type Context<'a> = NativeContext<'a, NodeHost>;
fn get(c: &mut Context<'_>, object: RootId, name: &str) -> Result<RootId, RootedError> {
    c.get_named_property_rooted(object, name)
}
fn put(c: &mut Context<'_>, object: RootId, name: &str, value: RootId) -> Result<(), RootedError> {
    let key = c.string_rooted(name);
    c.define_data_property_rooted(object, key, value, true, true, true)?;
    Ok(())
}
// Node captures its validators and coded constructors before executing guest code.
const PUBLIC_POLICY: &str = "\0buffer/originalPublicPolicy";
pub(crate) fn capture(c: &mut Context<'_>) -> Result<RootId, RootedError> {
    crate::host::shared_vm::commonjs::cached_native_binding(c, PUBLIC_POLICY, |c| {
        let policy = c.null_object_rooted()?;
        for name in [
            "validateNumber",
            "validateInteger",
            "validateArray",
            "validateString",
        ] {
            let f = crate::host::shared_vm::commonjs::node_library_export(
                c,
                "internal/validators.js",
                "internal/validators.js",
                name,
            )?;
            put(c, policy, name, f)?;
        }
        let codes = crate::host::shared_vm::commonjs::node_library_export(
            c,
            "internal/errors.js",
            "internal/errors.js",
            "codes",
        )?;
        for name in [
            "ERR_INVALID_ARG_VALUE",
            "ERR_OUT_OF_RANGE",
            "ERR_INVALID_ARG_TYPE",
        ] {
            let ctor = get(c, codes, name)?;
            put(c, policy, name, ctor)?;
        }
        let global = c.global_root()?;
        let number = get(c, global, "Number")?;
        let maximum = get(c, number, "MAX_SAFE_INTEGER")?;
        put(c, policy, "maximumLength", maximum)?;
        Ok(policy)
    })
}
pub(crate) fn initialize(c: &mut Context<'_>) -> Result<(), RootedError> {
    capture(c)?;
    Ok(())
}
