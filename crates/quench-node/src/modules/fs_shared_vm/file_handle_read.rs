//! FileHandle.read argument policy over configured validators and live buffers.
use super::get;
use crate::host::NodeHost;
use crate::modules::fs_policy_shared_vm as policy;
use quench_runtime::{BuiltinBrand, NativeContext, RootId, RootedError, Value};

type Context<'a> = NativeContext<'a, NodeHost>;
const DEFAULT_READ_BYTES: usize = 16 * 1024;

fn nullish(context: &Context<'_>, value: RootId) -> bool {
    context
        .rooted_value(value)
        .is_some_and(|v| v.is_null() || v.is_undefined())
}
fn missing(context: &Context<'_>, value: RootId) -> bool {
    context.rooted_value(value).is_some_and(Value::is_undefined)
}
fn option(context: &mut Context<'_>, options: RootId, name: &str) -> Result<RootId, RootedError> {
    if nullish(context, options) {
        Ok(context.undefined())
    } else {
        get(context, options, name)
    }
}
fn span(
    context: &mut Context<'_>,
    options: RootId,
    buffer: RootId,
) -> Result<[RootId; 3], RootedError> {
    let mut offset = option(context, options, "offset")?;
    if missing(context, offset) {
        offset = context.number(0.0);
    }
    let mut length = option(context, options, "length")?;
    if missing(context, length) {
        let capacity = get(context, buffer, "byteLength")?;
        length = context.subtract_rooted(capacity, offset)?;
    }
    let mut position = option(context, options, "position")?;
    if missing(context, position) {
        position = context.null();
    }
    Ok([offset, length, position])
}

/// Normalize the promise API before dispatching to the existing live descriptor
/// provider. The synchronous API then owns range validation and actual byte I/O.
pub(crate) fn parameters(
    context: &mut Context<'_>,
    _: RootId,
    args: &[RootId],
) -> Result<RootId, RootedError> {
    let undefined = context.undefined();
    let mut buffer = args.first().copied().unwrap_or(undefined);
    let mut offset = args.get(1).copied().unwrap_or(undefined);
    let mut length = args.get(2).copied().unwrap_or(undefined);
    let mut position = args.get(3).copied().unwrap_or(undefined);
    if !context.has_builtin_brand_rooted(buffer, BuiltinBrand::ArrayBufferView)? {
        let options = buffer;
        if !missing(context, options) {
            let name = context.string_rooted("options");
            let nullable = policy::export(context, "kValidateObjectAllowNullable")?;
            policy::call(context, "validateObject", &[options, name, nullable])?;
        }
        buffer = option(context, options, "buffer")?;
        if missing(context, buffer) {
            let constructor = policy::export(context, "Buffer")?;
            let alloc = get(context, constructor, "alloc")?;
            let size = context.number(DEFAULT_READ_BYTES as f64);
            buffer = context.call_rooted(alloc, constructor, &[size])?;
        }
        [offset, length, position] = span(context, options, buffer)?;
        policy::call(context, "validateBuffer", &[buffer])?;
    }
    if !context.rooted_value(offset).is_some_and(Value::is_null)
        && context.is_object_rooted(offset)?
    {
        [offset, length, position] = span(context, offset, buffer)?;
    }
    if nullish(context, offset) {
        offset = context.number(0.0);
    } else {
        let name = context.string_rooted("offset");
        let zero = context.number(0.0);
        policy::call(context, "validateInteger", &[offset, name, zero])?;
    }
    if nullish(context, length) {
        let capacity = get(context, buffer, "byteLength")?;
        length = context.subtract_rooted(capacity, offset)?;
    }
    let result = context.null_object_rooted()?;
    for (name, value) in [
        ("buffer", buffer),
        ("offset", offset),
        ("length", length),
        ("position", position),
    ] {
        let key = context.string_rooted(name);
        context.define_data_property_rooted(result, key, value, true, true, true)?;
    }
    Ok(result)
}
