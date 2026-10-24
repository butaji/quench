//! Node fill validation and pattern expansion over Core-owned typed arrays.
use crate::host::NodeHost;
use quench_runtime::{BuiltinBrand, NativeArithmetic, NativeContext, RootId, RootedError};
type Context<'a> = NativeContext<'a, NodeHost>;
// Node maps single ASCII code units to its numeric typed-array fill path.
const ASCII_SINGLE_UNIT_LIMIT: u16 = 0x80;
fn get(c: &mut Context<'_>, object: RootId, name: &str) -> Result<RootId, RootedError> {
    let key = c.string_rooted(name);
    c.get_property_rooted(object, key)
}
fn put(c: &mut Context<'_>, object: RootId, name: &str, value: RootId) -> Result<(), RootedError> {
    let key = c.string_rooted(name);
    c.define_data_property_rooted(object, key, value, false, false, false)?;
    Ok(())
}
fn undefined(c: &Context<'_>, value: RootId) -> bool {
    c.rooted_value(value).is_some_and(|v| v.is_undefined())
}
fn numeric(c: &Context<'_>, value: RootId) -> Option<f64> {
    c.rooted_value(value).and_then(|v| v.as_number())
}
fn error(
    c: &mut Context<'_>,
    message: &str,
    code: &str,
    range: bool,
) -> Result<RootedError, RootedError> {
    let e = if range {
        c.range_error_rooted(message)?
    } else {
        c.type_error_rooted(message)?
    };
    let key = c.string_rooted("code");
    let value = c.string_rooted(code);
    c.set_property_rooted(e, key, value, e)?;
    Ok(c.throw(e))
}
fn out_of_bounds(c: &mut Context<'_>) -> Result<RootedError, RootedError> {
    error(
        c,
        "Attempt to access memory outside buffer bounds",
        "ERR_BUFFER_OUT_OF_BOUNDS",
        true,
    )
}
fn getter(c: &mut Context<'_>, prototype: RootId, name: &str) -> Result<RootId, RootedError> {
    let key = c.string_rooted(name);
    let descriptor = c.own_property_descriptor_rooted(prototype, key)?;
    get(c, descriptor, "get")
}
fn intrinsics(c: &mut Context<'_>) -> Result<RootId, RootedError> {
    let global = c.global_root()?;
    let u8 = get(c, global, "Uint8Array")?;
    let prototype = get(c, u8, "prototype")?;
    let typed = c.prototype_rooted(prototype)?;
    let dv = get(c, global, "DataView")?;
    let dv = get(c, dv, "prototype")?;
    let data = c.null_object_rooted()?;
    put(c, data, "Uint8Array", u8)?;
    for (name, property) in [
        ("typedLength", "byteLength"),
        ("typedBuffer", "buffer"),
        ("typedOffset", "byteOffset"),
    ] {
        let value = getter(c, typed, property)?;
        put(c, data, name, value)?;
    }
    for (name, property) in [
        ("viewLength", "byteLength"),
        ("viewBuffer", "buffer"),
        ("viewOffset", "byteOffset"),
    ] {
        let value = getter(c, dv, property)?;
        put(c, data, name, value)?;
    }
    for name in ["fill", "set", "copyWithin"] {
        let value = get(c, typed, name)?;
        put(c, data, name, value)?;
    }
    Ok(data)
}
fn function(c: &mut Context<'_>, binding: bool) -> Result<RootId, RootedError> {
    let data = intrinsics(c)?;
    let function = if binding {
        c.host_function_with_data(crate::host::shared_vm::operation("bufferBindingFill"), data)?
    } else {
        c.host_constructor_with_data(crate::host::shared_vm::operation("bufferFill"), data)?
    };
    let key = c.string_rooted("name");
    let name = c.string_rooted("fill");
    c.define_data_property_rooted(function, key, name, false, false, true)?;
    Ok(function)
}
pub(crate) fn install(c: &mut Context<'_>, prototype: RootId) -> Result<(), RootedError> {
    let function = function(c, false)?;
    let key = c.string_rooted("fill");
    c.define_data_property_rooted(prototype, key, function, true, true, true)?;
    Ok(())
}
pub(crate) fn binding(c: &mut Context<'_>) -> Result<RootId, RootedError> {
    const CACHE_KEY: &str = "\0binding:buffer";
    let shared = c.host_mut().shared_state();
    if let Some(binding) = shared.borrow().builtin_cache.get(CACHE_KEY).copied() {
        return Ok(binding);
    }
    let binding = c.with_startup_intrinsics(|c| {
        let result = c.object_rooted()?;
        super::buffer_alloc_shared_vm::install(c, result)?;
        let function = function(c, true)?;
        let key = c.string_rooted("fill");
        c.define_data_property_rooted(result, key, function, true, true, true)?;
        let compare = c.host_function(crate::host::shared_vm::operation("bufferCompareRaw"))?;
        let key = c.string_rooted("name");
        let name = c.string_rooted("compare");
        c.define_data_property_rooted(compare, key, name, false, false, true)?;
        let key = c.string_rooted("compare");
        c.define_data_property_rooted(result, key, compare, true, true, true)?;
        let data = intrinsics(c)?;
        let copy = c.host_function_with_data(
            crate::host::shared_vm::operation("bufferCopyArrayBuffer"),
            data,
        )?;
        let key = c.string_rooted("name");
        let name = c.string_rooted("copyArrayBuffer");
        c.define_data_property_rooted(copy, key, name, false, false, true)?;
        let key = c.string_rooted("copyArrayBuffer");
        c.define_data_property_rooted(result, key, copy, true, true, true)?;
        Ok(result)
    })?;
    let binding = c.retain(binding)?;
    shared
        .borrow_mut()
        .builtin_cache
        .insert(CACHE_KEY.to_owned(), binding);
    Ok(binding)
}

fn normalize_encoding(c: &mut Context<'_>, encoding: RootId) -> Result<&'static str, RootedError> {
    if c.rooted_value(encoding)
        .is_some_and(|v| v.is_null() || v.is_undefined())
    {
        return Ok("utf8");
    }
    if c.string_text(encoding)?.as_deref() == Some("") {
        return Ok("utf8");
    }
    let length = get(c, encoding, "length")?;
    if let Some(length) = numeric(c, length).filter(|length| {
        length.fract() == 0.0
            && *length >= "hex".len() as f64
            && *length <= "base64url".len() as f64
    }) {
        let text = c.to_string(encoding)?;
        if text.encode_utf16().count() as f64 == length {
            if let Some(name) = super::buffer_codec::canonical_encoding(&text) {
                return Ok(name);
            }
        }
    }
    if c.string_text(encoding)?.is_none() {
        return Err(super::util_shared_vm::argument_type_error(
            c, encoding, "encoding", "string",
        )?);
    }
    let text = c.to_string(encoding)?;
    Err(error(
        c,
        &format!("Unknown encoding: {text}"),
        "ERR_UNKNOWN_ENCODING",
        false,
    )?)
}
fn index(c: &mut Context<'_>, value: RootId) -> Result<usize, RootedError> {
    if undefined(c, value) {
        return Ok(0);
    }
    let number = c.to_number(value)?.trunc() as i64;
    if number < 0 {
        return Err(error(c, "Index out of range", "ERR_OUT_OF_RANGE", true)?);
    }
    Ok(number as usize)
}
fn raw_fill(
    c: &mut Context<'_>,
    data: RootId,
    receiver: RootId,
    value: RootId,
    start: RootId,
    end: RootId,
    encoding: RootId,
) -> Result<RootId, RootedError> {
    if !c.has_builtin_brand_rooted(receiver, BuiltinBrand::ArrayBufferView)? {
        return Err(error(
            c,
            "argument must be a buffer",
            "ERR_INVALID_ARG_TYPE",
            false,
        )?);
    }
    let view = c.has_builtin_brand_rooted(receiver, BuiltinBrand::DataView)?;
    let length_get = get(c, data, if view { "viewLength" } else { "typedLength" })?;
    let byte_length = c.call_rooted(length_get, receiver, &[])?;
    let byte_length =
        numeric(c, byte_length).ok_or_else(|| RootedError::host("view length missing"))? as usize;
    let start = index(c, start)?;
    let end = index(c, end)?;
    if start > end || end > byte_length {
        return Ok(c.number(-2.0));
    }
    let count = end - start;
    let pattern = if c.has_builtin_brand_rooted(value, BuiltinBrand::ArrayBufferView)? {
        c.view_bytes_rooted(value).unwrap_or_default()
    } else if let Some(units) = c.string_code_units(value)? {
        let name = c
            .string_text(encoding)?
            .and_then(|name| super::buffer_codec::canonical_encoding(&name))
            .unwrap_or("utf8");
        super::buffer_codec::encode_units(&units, name)
    } else {
        let value = c.to_number(value)?;
        let value = c.number(value);
        let mask = c.number(u8::MAX as f64);
        let byte = c.arithmetic_rooted(NativeArithmetic::BitwiseAnd, value, mask)?;
        vec![numeric(c, byte).ok_or_else(|| RootedError::host("numeric byte missing"))? as u8]
    };
    if count == 0 {
        return Ok(c.undefined());
    }
    if pattern.is_empty() {
        return Ok(c.number(-1.0));
    }
    let buffer_get = get(c, data, if view { "viewBuffer" } else { "typedBuffer" })?;
    let backing = c.call_rooted(buffer_get, receiver, &[])?;
    let offset_get = get(c, data, if view { "viewOffset" } else { "typedOffset" })?;
    let offset = c.call_rooted(offset_get, receiver, &[])?;
    let offset = numeric(c, offset).ok_or_else(|| RootedError::host("view offset missing"))?;
    let constructor = get(c, data, "Uint8Array")?;
    let offset = c.number(offset + start as f64);
    let length = c.number(count as f64);
    let destination = c.construct_rooted(constructor, constructor, &[backing, offset, length])?;
    let mut filled = pattern.len().min(count);
    let source = c.uint8_array_rooted(&pattern[..filled])?;
    let set = get(c, data, "set")?;
    c.call_rooted(set, destination, &[source])?;
    let copy = get(c, data, "copyWithin")?;
    while filled < count {
        let take = filled.min(count - filled);
        let target = c.number(filled as f64);
        let zero = c.number(0.0);
        let end = c.number(take as f64);
        c.call_rooted(copy, destination, &[target, zero, end])?;
        filled += take;
    }
    Ok(c.undefined())
}
pub(crate) fn fill(
    c: &mut Context<'_>,
    receiver: RootId,
    args: &[RootId],
) -> Result<RootId, RootedError> {
    let data = c.host_function_data()?;
    let missing = c.undefined();
    let original = args.first().copied().unwrap_or(missing);
    let mut value = original;
    let mut offset = args.get(1).copied().unwrap_or(missing);
    let mut end = args.get(2).copied().unwrap_or(missing);
    let mut encoding = args.get(3).copied().unwrap_or(missing);
    if let Some(units) = c.string_code_units(value)? {
        if undefined(c, offset) || c.string_text(offset)?.is_some() {
            encoding = offset;
            offset = c.number(0.0);
            end = get(c, receiver, "length")?;
        } else if c.string_text(end)?.is_some() {
            encoding = end;
            end = get(c, receiver, "length")?;
        }
        let name = normalize_encoding(c, encoding)?;
        encoding = c.string_rooted(name);
        if units.is_empty() {
            value = c.number(0.0);
        } else if units.len() == 1
            && ((matches!(name, "utf8" | "ascii") && units[0] < ASCII_SINGLE_UNIT_LIMIT)
                || name == "latin1")
        {
            value = c.number(f64::from(units[0]));
        }
    } else {
        encoding = missing;
    }
    if undefined(c, offset) {
        offset = c.number(0.0);
        end = get(c, receiver, "length")?;
    } else {
        let maximum = c.number(super::util_shared_vm::MAX_SAFE_INTEGER);
        super::util_shared_vm::validate_integer(c, offset, "offset", 0.0, maximum)?;
        if undefined(c, end) {
            end = get(c, receiver, "length")?;
        } else {
            let maximum = get(c, receiver, "length")?;
            super::util_shared_vm::validate_integer(c, end, "end", 0.0, maximum)?;
        }
        if c.greater_equal_rooted(offset, end)? {
            return Ok(receiver);
        }
    }
    if numeric(c, value).is_some() {
        let length_get = get(c, data, "typedLength")?;
        let byte_length = c.call_rooted(length_get, receiver, &[])?;
        let length = c.subtract_rooted(end, offset)?;
        let total = c.arithmetic_rooted(NativeArithmetic::Add, length, offset)?;
        if c.greater_than_rooted(offset, end)? || c.greater_than_rooted(total, byte_length)? {
            return Err(out_of_bounds(c)?);
        }
        let fill = get(c, data, "fill")?;
        c.call_rooted(fill, receiver, &[value, offset, end])?;
    } else {
        let result = raw_fill(c, data, receiver, value, offset, end, encoding)?;
        match numeric(c, result) {
            Some(-1.0) => {
                return Err(super::util_shared_vm::argument_value_error(
                    c,
                    original,
                    "value",
                    "is invalid",
                )?);
            }
            Some(-2.0) => return Err(out_of_bounds(c)?),
            _ => {}
        }
    }
    Ok(receiver)
}
pub(crate) fn binding_fill(
    c: &mut Context<'_>,
    _: RootId,
    args: &[RootId],
) -> Result<RootId, RootedError> {
    let data = c.host_function_data()?;
    let missing = c.undefined();
    let mut values = [missing; 5];
    for (to, from) in values.iter_mut().zip(args) {
        *to = *from;
    }
    raw_fill(
        c, data, values[0], values[1], values[2], values[3], values[4],
    )
}

/// Copy via Core's captured typed-array intrinsics, including overlapping stores.
pub(crate) fn copy_array_buffer(
    c: &mut Context<'_>,
    _: RootId,
    args: &[RootId],
) -> Result<RootId, RootedError> {
    let data = c.host_function_data()?;
    let constructor = get(c, data, "Uint8Array")?;
    let target = c.construct_rooted(constructor, constructor, &[args[0], args[1], args[4]])?;
    let source = c.construct_rooted(constructor, constructor, &[args[2], args[3], args[4]])?;
    let set = get(c, data, "set")?;
    c.call_rooted(set, target, &[source])?;
    Ok(c.undefined())
}
