//! Buffer's original custom inspector and shared public byte display limit.
use crate::host::NodeHost;
use quench_runtime::{BuiltinBrand, NativeContext, RootId, RootedError};
type Context<'a> = NativeContext<'a, NodeHost>;
pub(crate) const DEFAULT_INSPECT_MAX_BYTES: f64 = 50.0;
fn get(c: &mut Context<'_>, o: RootId, n: &str) -> Result<RootId, RootedError> {
    let k = c.string_rooted(n);
    c.get_property_rooted(o, k)
}
fn named(c: &mut Context<'_>, f: RootId, n: &str) -> Result<(), RootedError> {
    let k = c.string_rooted("name");
    let v = c.string_rooted(n);
    c.define_data_property_rooted(f, k, v, false, false, true)?;
    Ok(())
}
pub(crate) fn install(
    c: &mut Context<'_>,
    module: RootId,
    prototype: RootId,
) -> Result<(), RootedError> {
    let data = c.null_object_rooted()?;
    let key = c.string_rooted("maximum");
    let max = c.number(DEFAULT_INSPECT_MAX_BYTES);
    c.define_data_property_rooted(data, key, max, true, false, false)?;
    let function =
        c.host_constructor_with_data(crate::host::shared_vm::operation("bufferInspect"), data)?;
    named(c, function, "inspect")?;
    let key = c.string_rooted("inspect");
    c.define_data_property_rooted(prototype, key, function, true, true, true)?;
    let global = c.global_root()?;
    let u8 = get(c, global, "Uint8Array")?;
    let byte_prototype = get(c, u8, "prototype")?;
    let typed = c.prototype_rooted(byte_prototype)?;
    let view = get(c, global, "DataView")?;
    let view = get(c, view, "prototype")?;
    for (name, holder) in [("typedByteLength", typed), ("viewByteLength", view)] {
        let key = c.string_rooted("byteLength");
        let descriptor = c.own_property_descriptor_rooted(holder, key)?;
        let getter = get(c, descriptor, "get")?;
        let key = c.string_rooted(name);
        c.define_data_property_rooted(data, key, getter, false, false, false)?;
    }
    let symbol = get(c, global, "Symbol")?;
    let symbol_for = get(c, symbol, "for")?;
    let text = c.string_rooted("nodejs.util.inspect.custom");
    let key = c.call_rooted(symbol_for, symbol, &[text])?;
    c.define_data_property_rooted(prototype, key, function, true, true, true)?;
    let getter = c.host_function_with_data(
        crate::host::shared_vm::operation("bufferInspectMaxGet"),
        data,
    )?;
    named(c, getter, "get")?;
    let setter = c.host_function_with_data(
        crate::host::shared_vm::operation("bufferInspectMaxSet"),
        data,
    )?;
    named(c, setter, "set")?;
    let descriptor = c.null_object_rooted()?;
    for (name, value) in [("get", getter), ("set", setter)] {
        let key = c.string_rooted(name);
        c.define_data_property_rooted(descriptor, key, value, true, true, true)?;
    }
    for name in ["enumerable", "configurable"] {
        let key = c.string_rooted(name);
        let yes = c.boolean(true);
        c.define_data_property_rooted(descriptor, key, yes, true, true, true)?;
    }
    let key = c.string_rooted("INSPECT_MAX_BYTES");
    c.define_property_descriptor_rooted(module, key, descriptor)?;
    Ok(())
}
pub(crate) fn maximum(c: &mut Context<'_>, _: RootId, _: &[RootId]) -> Result<RootId, RootedError> {
    let data = c.host_function_data()?;
    get(c, data, "maximum")
}
pub(crate) fn set_maximum(
    c: &mut Context<'_>,
    _: RootId,
    args: &[RootId],
) -> Result<RootId, RootedError> {
    let value = args.first().copied().unwrap_or_else(|| c.undefined());
    let Some(number) = c.rooted_value(value).and_then(|v| v.as_number()) else {
        return Err(super::util_shared_vm::argument_type_error(
            c,
            value,
            "INSPECT_MAX_BYTES",
            "number",
        )?);
    };
    if number.is_nan() || number < 0.0 {
        let received = super::util_shared_vm::range_error_received(c, value)?;
        let e=c.range_error_rooted(&format!("The value of \"INSPECT_MAX_BYTES\" is out of range. It must be >= 0. Received {received}"))?;
        let key = c.string_rooted("code");
        let code = c.string_rooted("ERR_OUT_OF_RANGE");
        c.set_property_rooted(e, key, code, e)?;
        return Err(c.throw(e));
    }
    let data = c.host_function_data()?;
    let key = c.string_rooted("maximum");
    c.set_property_rooted(data, key, value, data)?;
    Ok(c.undefined())
}
pub(crate) fn inspect(
    c: &mut Context<'_>,
    receiver: RootId,
    args: &[RootId],
) -> Result<RootId, RootedError> {
    let data = c.host_function_data()?;
    let max = get(c, data, "maximum")?;
    let max = c
        .rooted_value(max)
        .and_then(|v| v.as_number())
        .ok_or_else(|| RootedError::host("Buffer inspect maximum missing"))?;
    let length = get(c, receiver, "length")?;
    let length = c.to_number(length)?;
    let maximum = if length.is_nan() {
        f64::NAN
    } else {
        max.min(length)
    };
    let remaining_root = get(c, receiver, "length")?;
    let remaining = c.to_number(remaining_root)? - max;
    if !c.has_builtin_brand_rooted(receiver, BuiltinBrand::ArrayBufferView)? {
        let e = c.type_error_rooted("argument must be a buffer")?;
        let key = c.string_rooted("code");
        let code = c.string_rooted("ERR_INVALID_ARG_TYPE");
        c.set_property_rooted(e, key, code, e)?;
        return Err(c.throw(e));
    }
    let view = c.has_builtin_brand_rooted(receiver, BuiltinBrand::DataView)?;
    let getter = get(
        c,
        data,
        if view {
            "viewByteLength"
        } else {
            "typedByteLength"
        },
    )?;
    let actual = c.call_rooted(getter, receiver, &[])?;
    let actual = c
        .rooted_value(actual)
        .and_then(|v| v.as_number())
        .ok_or_else(|| RootedError::host("view byte length missing"))? as usize;
    let limit = if actual == 0 {
        0
    } else {
        let index = maximum.trunc() as i64;
        if index < 0 || index as usize > actual {
            let error = c.range_error_rooted("Index out of range")?;
            let key = c.string_rooted("code");
            let code = c.string_rooted("ERR_OUT_OF_RANGE");
            c.set_property_rooted(error, key, code, error)?;
            return Err(c.throw(error));
        }
        index as usize
    };
    let Some(bytes) = c.bytes_prefix_rooted(receiver, limit) else {
        let e = c.type_error_rooted("argument must be a buffer")?;
        let key = c.string_rooted("code");
        let code = c.string_rooted("ERR_INVALID_ARG_TYPE");
        c.set_property_rooted(e, key, code, e)?;
        return Err(c.throw(e));
    };
    let mut text = bytes
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect::<Vec<_>>()
        .join(" ");
    if remaining > 0.0 {
        let n = c.number(remaining);
        let n = c.to_string(n)?;
        text.push_str(&format!(
            " ... {n} more byte{}",
            if remaining > 1.0 { "s" } else { "" }
        ));
    }
    let missing = c.undefined();
    let settings = args.get(1).copied().unwrap_or(missing);
    if c.truthy_rooted(settings)? {
        let hidden = get(c, settings, "showHidden")?;
        let hidden = c.truthy_rooted(hidden)?;
        let extras = c.null_object_rooted()?;
        let mut has_extras = false;
        for key in c.own_non_index_keys_rooted(receiver, !hidden)? {
            let d = c.own_property_descriptor_rooted(receiver, key)?;
            let enumerable = get(c, d, "enumerable")?;
            if !hidden && !c.truthy_rooted(enumerable)? {
                continue;
            }
            let value = c.get_property_rooted(receiver, key)?;
            c.define_data_property_rooted(extras, key, value, true, true, true)?;
            has_extras = true;
        }
        if has_extras {
            let options = c.null_object_rooted()?;
            for key in c.enumerable_own_keys_rooted(settings, true, true)? {
                let value = c.get_property_rooted(settings, key)?;
                c.define_data_property_rooted(options, key, value, true, true, true)?;
            }
            let key = c.string_rooted("breakLength");
            let infinity = c.number(f64::INFINITY);
            c.define_data_property_rooted(options, key, infinity, true, true, true)?;
            let key = c.string_rooted("compact");
            let yes = c.boolean(true);
            c.define_data_property_rooted(options, key, yes, true, true, true)?;
            let inspect = super::util_shared_vm::internal_inspect(c)?;
            let rendered = c.call_rooted(inspect, missing, &[extras, options])?;
            let rendered = c.to_string(rendered)?;
            // Node embeds the canonical property-list rendering inside its Buffer label.
            let properties = rendered
                .strip_prefix("[Object: null prototype] ")
                .unwrap_or(&rendered);
            let properties = properties
                .strip_prefix("{ ")
                .and_then(|v| v.strip_suffix(" }"))
                .unwrap_or(properties);
            if length != 0.0 {
                text.push_str(", ");
            }
            text.push_str(properties);
        }
    }
    let fallback = c.string_rooted("Buffer");
    let constructor_name = (|| -> Result<RootId, RootedError> {
        let constructor = get(c, receiver, "constructor")?;
        if !c.is_callable_rooted(constructor)? {
            return Ok(fallback);
        }
        let key = c.string_rooted("name");
        let descriptor = c.own_property_descriptor_rooted(constructor, key)?;
        if c.rooted_value(descriptor).is_some_and(|v| v.is_undefined()) {
            Ok(fallback)
        } else {
            c.get_property_rooted(constructor, key)
        }
    })()
    .unwrap_or(fallback);
    let name = c.to_string(constructor_name)?;
    Ok(c.string_rooted(&format!("<{name} {text}>")))
}
