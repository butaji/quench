//! Node's LazyTransform defers stream state until it is observed.
use crate::host::NodeHost;
use quench_runtime::{DescriptorFields, NativeContext, RootId, RootedError};
type Context<'a> = NativeContext<'a, NodeHost>;

pub(crate) fn constructor(c: &mut Context<'_>, transform: RootId) -> Result<RootId, RootedError> {
    let operation = crate::host::shared_vm::operation;
    let lazy = c.host_constructor_named(operation("cryptoLazyTransform"), "LazyTransform")?;
    c.set_prototype_rooted(lazy, transform)?;
    let prototype = c.get_named_property_rooted(lazy, "prototype")?;
    let transform_prototype = c.get_named_property_rooted(transform, "prototype")?;
    c.set_prototype_rooted(prototype, transform_prototype)?;
    for name in ["_readableState", "_writableState"] {
        let name = c.string_rooted(name);
        let data = c.record_rooted(false, &["transform", "name"], &[transform, name])?;
        let getter = c.host_function_named_with_data(operation("cryptoLazyStateGet"), "", data)?;
        let setter = c.host_function_named_with_data(operation("cryptoLazyStateSet"), "", data)?;
        c.define_property_fields_rooted(
            prototype,
            name,
            DescriptorFields {
                get: Some(getter),
                set: Some(setter),
                enumerable: Some(true),
                configurable: Some(true),
                ..Default::default()
            },
        )?;
    }
    Ok(lazy)
}
pub(crate) fn construct(
    c: &mut Context<'_>,
    this: RootId,
    args: &[RootId],
) -> Result<RootId, RootedError> {
    let options = args.first().copied().unwrap_or_else(|| c.undefined());
    c.assign_named_property_rooted(this, "_options", options)?;
    Ok(c.undefined())
}
pub(crate) fn get(c: &mut Context<'_>, this: RootId, _: &[RootId]) -> Result<RootId, RootedError> {
    let data = c.host_function_data()?;
    let transform = c.get_named_property_rooted(data, "transform")?;
    let options = c.get_named_property_rooted(this, "_options")?;
    c.call_rooted(transform, this, &[options])?;
    let writable = c.get_named_property_rooted(this, "_writableState")?;
    let no = c.boolean(false);
    c.assign_named_property_rooted(writable, "decodeStrings", no)?;
    let name = c.get_named_property_rooted(data, "name")?;
    c.get_property_rooted(this, name)
}
pub(crate) fn set(
    c: &mut Context<'_>,
    this: RootId,
    args: &[RootId],
) -> Result<RootId, RootedError> {
    let data = c.host_function_data()?;
    let name = c.get_named_property_rooted(data, "name")?;
    let value = args.first().copied().unwrap_or_else(|| c.undefined());
    c.define_data_property_rooted(this, name, value, true, true, true)?;
    Ok(c.undefined())
}
