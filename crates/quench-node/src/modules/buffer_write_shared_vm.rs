//! Buffer string writes preserve Node overload policy over Core-owned backing stores.
use crate::host::NodeHost;
use quench_runtime::{NativeArithmetic, NativeContext, RootId, RootedError};
type Context<'a> = NativeContext<'a, NodeHost>;
fn get(c: &mut Context<'_>, object: RootId, name: &str) -> Result<RootId, RootedError> {
    let key = c.string_rooted(name);
    c.get_property_rooted(object, key)
}
fn getter(c: &mut Context<'_>, prototype: RootId, name: &str) -> Result<RootId, RootedError> {
    let key = c.string_rooted(name);
    let descriptor = c.own_property_descriptor_rooted(prototype, key)?;
    get(c, descriptor, "get")
}
fn number(c: &Context<'_>, value: RootId) -> Option<f64> {
    c.rooted_value(value).and_then(|v| v.as_number())
}
fn undefined(c: &Context<'_>, value: RootId) -> bool {
    c.rooted_value(value).is_some_and(|v| v.is_undefined())
}
fn coded(
    c: &mut Context<'_>,
    message: &str,
    code: &str,
    range: bool,
) -> Result<RootedError, RootedError> {
    let error = if range {
        c.range_error_rooted(message)?
    } else {
        c.type_error_rooted(message)?
    };
    let key = c.string_rooted("code");
    let value = c.string_rooted(code);
    c.set_property_rooted(error, key, value, error)?;
    Ok(c.throw(error))
}
fn validate_offset(
    c: &mut Context<'_>,
    value: RootId,
    name: &str,
    maximum: usize,
) -> Result<usize, RootedError> {
    let Some(n) = number(c, value) else {
        return Err(super::util_shared_vm::argument_type_error(
            c, value, name, "number",
        )?);
    };
    if !n.is_finite() || n.fract() != 0.0 || n < 0.0 || n > maximum as f64 {
        let requirement = if !n.is_finite() || n.fract() != 0.0 {
            "an integer".to_owned()
        } else {
            format!(">= 0 && <= {maximum}")
        };
        let received = super::util_shared_vm::range_error_received(c, value)?;
        return Err(coded(
            c,
            &format!(
                "The value of \"{name}\" is out of range. It must be {requirement}. Received {received}"
            ),
            "ERR_OUT_OF_RANGE",
            true,
        )?);
    }
    Ok(n as usize)
}
pub(crate) fn install(c: &mut Context<'_>, prototype: RootId) -> Result<(), RootedError> {
    let global = c.global_root()?;
    let constructor = get(c, global, "Uint8Array")?;
    let byte_prototype = get(c, constructor, "prototype")?;
    let typed = c.prototype_rooted(byte_prototype)?;
    let length = getter(c, typed, "length")?;
    let buffer = getter(c, typed, "buffer")?;
    let offset = getter(c, typed, "byteOffset")?;
    let set = get(c, typed, "set")?;
    let data = c.array_rooted(&[length, buffer, offset, constructor, set])?;
    let function =
        c.host_constructor_with_data(crate::host::shared_vm::operation("bufferWrite"), data)?;
    let name = c.string_rooted("name");
    let value = c.string_rooted("write");
    c.define_data_property_rooted(function, name, value, false, false, true)?;
    let key = c.string_rooted("write");
    c.define_data_property_rooted(prototype, key, function, true, true, true)?;
    Ok(())
}
pub(crate) fn write(
    c: &mut Context<'_>,
    receiver: RootId,
    args: &[RootId],
) -> Result<RootId, RootedError> {
    let data = c.host_function_data()?;
    let length_getter = get(c, data, "0")?;
    let length = c.call_rooted(length_getter, receiver, &[])?;
    let buffer_length =
        number(c, length).ok_or_else(|| RootedError::host("typed-array length missing"))? as usize;
    let missing = c.undefined();
    let input = args.first().copied().unwrap_or(missing);
    let offset_arg = args.get(1).copied().unwrap_or(missing);
    let length_arg = args.get(2).copied().unwrap_or(missing);
    let mut encoding = args.get(3).copied().unwrap_or(missing);
    let (offset, length) = if undefined(c, offset_arg) {
        encoding = missing;
        (0, buffer_length)
    } else if undefined(c, length_arg) && c.string_text(offset_arg)?.is_some() {
        encoding = offset_arg;
        (0, buffer_length)
    } else {
        let offset = validate_offset(c, offset_arg, "offset", buffer_length)?;
        let remaining = buffer_length - offset;
        let length = if undefined(c, length_arg) {
            remaining
        } else if c.string_text(length_arg)?.is_some() {
            encoding = length_arg;
            remaining
        } else {
            validate_offset(c, length_arg, "length", buffer_length)?.min(remaining)
        };
        (offset, length)
    };
    let encoding_name = if !c.truthy_rooted(encoding)? {
        "utf8".to_owned()
    } else {
        let empty = c.string_rooted("");
        let text = c.arithmetic_rooted(NativeArithmetic::Add, encoding, empty)?;
        c.to_string(text)?
    };
    let Some(encoding_name) = super::buffer_codec::canonical_encoding(&encoding_name) else {
        let formatters = super::util_shared_vm::internal_formatters(c)?;
        let format = get(c, formatters, "format")?;
        let template = c.string_rooted("%s");
        let text = c.call_rooted(format, missing, &[template, encoding])?;
        let text = c.to_string(text)?;
        return Err(coded(
            c,
            &format!("Unknown encoding: {text}"),
            "ERR_UNKNOWN_ENCODING",
            false,
        )?);
    };
    if matches!(encoding_name, "utf8" | "ascii" | "latin1") {
        let byte_length = get(c, receiver, "byteLength")?;
        let offset_value = c.number(offset as f64);
        if c.greater_than_rooted(offset_value, byte_length)? {
            return Err(coded(
                c,
                "\"offset\" is outside of buffer bounds",
                "ERR_BUFFER_OUT_OF_BOUNDS",
                true,
            )?);
        }
        let byte_length = get(c, receiver, "byteLength")?;
        let remaining = c.subtract_rooted(byte_length, offset_value)?;
        let remaining =
            number(c, remaining).ok_or_else(|| RootedError::host("numeric byte length missing"))?;
        if length as f64 > remaining {
            return Err(coded(
                c,
                "\"length\" is outside of buffer bounds",
                "ERR_BUFFER_OUT_OF_BOUNDS",
                true,
            )?);
        }
    }
    let Some(units) = c.string_code_units(input)? else {
        return Err(coded(
            c,
            "argument must be a string",
            "ERR_INVALID_ARG_TYPE",
            false,
        )?);
    };
    let mut bytes = super::buffer_codec::encode_units(&units, encoding_name);
    let mut count = length.min(bytes.len());
    if encoding_name == "utf8" {
        let text = std::str::from_utf8(&bytes)
            .map_err(|_| RootedError::host("UTF-8 codec produced invalid bytes"))?;
        while !text.is_char_boundary(count) {
            count -= 1;
        }
    } else if encoding_name == "utf16le" {
        count -= count % std::mem::size_of::<u16>();
    }
    if count == 0 {
        return Ok(c.number(0.0));
    }
    let buffer_getter = get(c, data, "1")?;
    let backing = c.call_rooted(buffer_getter, receiver, &[])?;
    let offset_getter = get(c, data, "2")?;
    let base = c.call_rooted(offset_getter, receiver, &[])?;
    let base = number(c, base).ok_or_else(|| RootedError::host("typed-array offset missing"))?;
    let start = c.number(base + offset as f64);
    let size = c.number(count as f64);
    let constructor = get(c, data, "3")?;
    let destination = c.construct_rooted(constructor, constructor, &[backing, start, size])?;
    bytes.truncate(count);
    let source = c.uint8_array_rooted(&bytes)?;
    let set = get(c, data, "4")?;
    c.call_rooted(set, destination, &[source])?;
    Ok(c.number(count as f64))
}
