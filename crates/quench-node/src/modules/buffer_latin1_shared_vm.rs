//! Latin-1 string classification over genuine Core UTF-16 data.
use crate::host::NodeHost;
use quench_runtime::{NativeContext, RootId, RootedError};
type Context<'a> = NativeContext<'a, NodeHost>;
pub(crate) fn install(c: &mut Context<'_>, module: RootId) -> Result<(), RootedError> {
    let policy = super::buffer_policy_shared_vm::capture(c)?;
    let key = c.string_rooted("validateString");
    let validate = c.get_property_rooted(policy, key)?;
    let function = c.host_constructor_with_data(
        crate::host::shared_vm::operation("bufferIsLatin1"),
        validate,
    )?;
    let name = c.string_rooted("name");
    let value = c.string_rooted("isLatin1");
    c.define_data_property_rooted(function, name, value, false, false, true)?;
    let name = c.string_rooted("isLatin1");
    c.define_data_property_rooted(module, name, function, true, true, true)?;
    Ok(())
}
fn classify(c: &mut Context<'_>, value: RootId) -> Result<RootId, RootedError> {
    let units = c
        .string_code_units(value)?
        .ok_or_else(|| RootedError::host("Latin-1 classification requires a string"))?;
    Ok(c.boolean(units.iter().all(|unit| *unit <= u8::MAX as u16)))
}
pub(crate) fn public(
    c: &mut Context<'_>,
    _: RootId,
    args: &[RootId],
) -> Result<RootId, RootedError> {
    let value = args.first().copied().unwrap_or_else(|| c.undefined());
    let validate = c.host_function_data()?;
    let undefined = c.undefined();
    let name = c.string_rooted("input");
    c.call_rooted(validate, undefined, &[value, name])?;
    classify(c, value)
}
pub(crate) fn binding(
    c: &mut Context<'_>,
    _: RootId,
    args: &[RootId],
) -> Result<RootId, RootedError> {
    let value = args.first().copied().unwrap_or_else(|| c.undefined());
    classify(c, value)
}
