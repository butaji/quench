//! Actual backing allocations and physical alignment for the Node Buffer ABI.
use crate::host::NodeHost;
use quench_runtime::{NativeContext, RootId, RootedError};
type Context<'a> = NativeContext<'a, NodeHost>;
fn get(c: &mut Context<'_>, o: RootId, n: &str) -> Result<RootId, RootedError> {
    let k = c.string_rooted(n);
    c.get_property_rooted(o, k)
}
fn put(c: &mut Context<'_>, o: RootId, n: &str, v: RootId) -> Result<(), RootedError> {
    let k = c.string_rooted(n);
    c.define_data_property_rooted(o, k, v, true, true, true)?;
    Ok(())
}
pub(crate) fn install(c: &mut Context<'_>, binding: RootId) -> Result<(), RootedError> {
    let global = c.global_root()?;
    let ctor = get(c, global, "ArrayBuffer")?;
    let allocate = c.host_function_with_data(
        crate::host::shared_vm::operation("createUnsafeArrayBuffer"),
        ctor,
    )?;
    put(c, binding, "createUnsafeArrayBuffer", allocate)?;
    let aligned = c.host_function(crate::host::shared_vm::operation(
        "arrayBufferAlignedOffset",
    ))?;
    put(c, binding, "arrayBufferAlignedOffset", aligned)?;
    let latin1 = c.host_function_named(
        crate::host::shared_vm::operation("bufferBindingIsLatin1"),
        "",
    )?;
    put(c, binding, "isLatin1", latin1)?;
    Ok(())
}
pub(crate) fn allocate(
    c: &mut Context<'_>,
    _: RootId,
    args: &[RootId],
) -> Result<RootId, RootedError> {
    let ctor = c.host_function_data()?;
    if args.len() != 1 {
        let e = c.range_error_rooted("Invalid array buffer length")?;
        return Err(c.throw(e));
    }
    // Zero-initialized storage is permitted for the unsafe allocation API and
    // keeps every byte initialized in Rust. The allocation and lifetime are real.
    c.construct_rooted(ctor, ctor, args)
}
pub(crate) fn aligned_offset(
    c: &mut Context<'_>,
    _: RootId,
    args: &[RootId],
) -> Result<RootId, RootedError> {
    let buffer = *args
        .first()
        .ok_or_else(|| RootedError::host("ArrayBuffer argument missing"))?;
    let alignment = *args
        .get(1)
        .ok_or_else(|| RootedError::host("Alignment argument missing"))?;
    let alignment = c.to_number(alignment)?;
    if !alignment.is_finite()
        || alignment <= 0.0
        || alignment.fract() != 0.0
        || alignment >= usize::MAX as f64
    {
        let e = c.range_error_rooted("Invalid buffer alignment")?;
        return Err(c.throw(e));
    }
    let offset = c.array_buffer_aligned_offset_rooted(buffer, alignment as usize)?;
    Ok(c.number(offset as f64))
}
