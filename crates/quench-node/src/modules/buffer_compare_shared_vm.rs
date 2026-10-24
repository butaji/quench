//! Buffer's static comparison validates real byte views, independent of prototypes.
use crate::host::NodeHost;
use quench_runtime::{BuiltinBrand, NativeContext, RootId, RootedError};

pub(crate) fn install(
    context: &mut NativeContext<'_, NodeHost>,
    constructor: RootId,
) -> Result<(), RootedError> {
    let function = context.host_constructor(crate::host::shared_vm::operation("bufferCompare"))?;
    let key = context.string_rooted("name");
    let name = context.string_rooted("compare");
    context.define_data_property_rooted(function, key, name, false, false, true)?;
    let key = context.string_rooted("compare");
    context.define_data_property_rooted(constructor, key, function, true, true, true)?;
    let key = context.string_rooted("prototype");
    let prototype = context.get_property_rooted(constructor, key)?;
    let byte_prototype = context.prototype_rooted(prototype)?;
    let typed_prototype = context.prototype_rooted(byte_prototype)?;
    let key = context.string_rooted("byteLength");
    let descriptor = context.own_property_descriptor_rooted(typed_prototype, key)?;
    let key = context.string_rooted("get");
    let getter = context.get_property_rooted(descriptor, key)?;
    let equals = context
        .host_constructor_with_data(crate::host::shared_vm::operation("bufferEquals"), getter)?;
    let key = context.string_rooted("name");
    let name = context.string_rooted("equals");
    context.define_data_property_rooted(equals, key, name, false, false, true)?;
    let key = context.string_rooted("equals");
    context.define_data_property_rooted(prototype, key, equals, true, true, true)?;
    Ok(())
}

pub(crate) fn equals(
    context: &mut NativeContext<'_, NodeHost>,
    receiver: RootId,
    args: &[RootId],
) -> Result<RootId, RootedError> {
    let other = args.first().copied().unwrap_or_else(|| context.undefined());
    if !context.has_builtin_brand_rooted(other, BuiltinBrand::Uint8Array)? {
        return Err(super::util_shared_vm::argument_type_error_with_expected(
            context,
            other,
            "otherBuffer",
            "an instance of Buffer or Uint8Array",
        )?);
    }
    if context.rooted_value(receiver) == context.rooted_value(other) {
        return Ok(context.boolean(true));
    }
    let getter = context.host_function_data()?;
    let left_length = context.call_rooted(getter, receiver, &[])?;
    let right_length = context.call_rooted(getter, other, &[])?;
    let left_length = context
        .rooted_value(left_length)
        .and_then(|value| value.as_number());
    let right_length = context
        .rooted_value(right_length)
        .and_then(|value| value.as_number());
    if left_length != right_length {
        return Ok(context.boolean(false));
    }
    if left_length == Some(0.0) {
        return Ok(context.boolean(true));
    }
    let left = context.view_bytes_rooted(receiver).unwrap_or_default();
    let right = context.view_bytes_rooted(other).unwrap_or_default();
    Ok(context.boolean(left == right))
}

pub(crate) fn compare(
    context: &mut NativeContext<'_, NodeHost>,
    _: RootId,
    args: &[RootId],
) -> Result<RootId, RootedError> {
    let absent = context.undefined();
    let left = args.first().copied().unwrap_or(absent);
    let right = args.get(1).copied().unwrap_or(absent);
    for (name, value) in [("buf1", left), ("buf2", right)] {
        if !context.has_builtin_brand_rooted(value, BuiltinBrand::Uint8Array)? {
            return Err(super::util_shared_vm::argument_type_error_with_expected(
                context,
                value,
                name,
                "an instance of Buffer or Uint8Array",
            )?);
        }
    }
    if context.rooted_value(left) == context.rooted_value(right) {
        return Ok(context.number(0.0));
    }
    let left = context.view_bytes_rooted(left).unwrap_or_default();
    let right = context.view_bytes_rooted(right).unwrap_or_default();
    Ok(context.number(order(&left, &right)))
}

fn order(left: &[u8], right: &[u8]) -> f64 {
    match left.cmp(right) {
        std::cmp::Ordering::Less => -1.0,
        std::cmp::Ordering::Equal => 0.0,
        std::cmp::Ordering::Greater => 1.0,
    }
}

/// The native comparison ABI also consumes non-byte typed arrays and DataViews.
pub(crate) fn raw_compare(
    context: &mut NativeContext<'_, NodeHost>,
    _: RootId,
    args: &[RootId],
) -> Result<RootId, RootedError> {
    let missing = context.undefined();
    let left = args.first().copied().unwrap_or(missing);
    let right = args.get(1).copied().unwrap_or(missing);
    let Some(left) = context.view_bytes_rooted(left) else {
        let error =
            context.type_error_rooted("First comparison argument must have byte storage")?;
        return Err(context.throw(error));
    };
    let Some(right) = context.view_bytes_rooted(right) else {
        let error =
            context.type_error_rooted("Second comparison argument must have byte storage")?;
        return Err(context.throw(error));
    };
    Ok(context.number(order(&left, &right)))
}
