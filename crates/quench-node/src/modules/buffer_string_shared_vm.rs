//! Native Buffer string conversion over original Core backing stores.
use crate::host::NodeHost;
use quench_runtime::{BuiltinBrand, NativeArithmetic, NativeContext, RootId, RootedError};
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
fn arg(c: &mut Context<'_>, args: &[RootId], i: usize) -> RootId {
    args.get(i).copied().unwrap_or_else(|| c.undefined())
}
fn undefined(c: &Context<'_>, v: RootId) -> bool {
    c.rooted_value(v).is_some_and(|v| v.is_undefined())
}
fn getter(c: &mut Context<'_>, p: RootId, n: &str) -> Result<RootId, RootedError> {
    let k = c.string_rooted(n);
    let d = c.own_property_descriptor_rooted(p, k)?;
    get(c, d, "get")
}
fn intrinsic(c: &mut Context<'_>, d: RootId, n: &str, v: RootId) -> Result<RootId, RootedError> {
    let f = get(c, d, n)?;
    c.call_rooted(f, v, &[])
}
fn out_of_range(c: &mut Context<'_>) -> Result<RootedError, RootedError> {
    let e = c.range_error_rooted("Index out of range")?;
    super::node_errors_shared_vm::decorate_range_error(c, e, "ERR_OUT_OF_RANGE")?;
    Ok(c.throw(e))
}
fn index(c: &mut Context<'_>, v: RootId, fallback: usize) -> Result<usize, RootedError> {
    if undefined(c, v) {
        return Ok(fallback);
    }
    // Node's native ParseArrayIndex uses V8 IntegerValue's truncating,
    // saturating signed conversion, then rejects negative indices.
    let n = c.to_number(v)? as i64;
    if n < 0 {
        return Err(out_of_range(c)?);
    }
    Ok(n as usize)
}
pub(crate) fn install(c: &mut Context<'_>, prototype: RootId) -> Result<(), RootedError> {
    let data = c.null_object_rooted()?;
    let global = c.global_root()?;
    let u8 = get(c, global, "Uint8Array")?;
    let p = get(c, u8, "prototype")?;
    let typed = c.prototype_rooted(p)?;
    put(c, data, "u8", u8)?;
    for n in ["length", "byteLength", "byteOffset", "buffer"] {
        let f = getter(c, typed, n)?;
        put(c, data, n, f)?;
    }
    let dv = get(c, global, "DataView")?;
    let p = get(c, dv, "prototype")?;
    for n in ["byteLength", "byteOffset", "buffer"] {
        let f = getter(c, p, n)?;
        put(c, data, &format!("dv{n}"), f)?;
    }
    let f =
        c.host_constructor_with_data(crate::host::shared_vm::operation("bufferToString"), data)?;
    let k = c.string_rooted("name");
    let n = c.string_rooted("toString");
    c.define_data_property_rooted(f, k, n, false, false, true)?;
    put(c, prototype, "toString", f)?;
    put(c, prototype, "toLocaleString", f)?;
    for (n, encoding) in [
        ("asciiSlice", "ascii"),
        ("base64Slice", "base64"),
        ("base64urlSlice", "base64url"),
        ("latin1Slice", "latin1"),
        ("hexSlice", "hex"),
        ("ucs2Slice", "utf16le"),
        ("utf8Slice", "utf8"),
    ] {
        let d = c.null_object_rooted()?;
        c.set_prototype_rooted(d, data)?;
        let encoding = c.string_rooted(encoding);
        put(c, d, "encoding", encoding)?;
        let f = c.host_constructor_with_data(
            crate::host::shared_vm::operation("bufferStringSlice"),
            d,
        )?;
        let key = c.string_rooted("name");
        let empty = c.string_rooted("");
        c.define_data_property_rooted(f, key, empty, false, false, true)?;
        put(c, prototype, n, f)?;
    }
    Ok(())
}
fn slice(
    c: &mut Context<'_>,
    data: RootId,
    receiver: RootId,
    start: RootId,
    end: RootId,
    encoding: &str,
) -> Result<RootId, RootedError> {
    if !c.has_builtin_brand_rooted(receiver, BuiltinBrand::ArrayBufferView)? {
        let e = c.type_error_rooted("argument must be a buffer")?;
        super::node_errors_shared_vm::decorate_type_error(c, e, "ERR_INVALID_ARG_TYPE")?;
        return Err(c.throw(e));
    }
    let dv = c.has_builtin_brand_rooted(receiver, BuiltinBrand::DataView)?;
    // Native Buffer slices see an empty backing view for detached or out-of-bounds
    // DataViews, whose JavaScript byteLength getter would instead throw.
    if dv
        && c.bytes_prefix_rooted(receiver, 1)
            .is_none_or(|bytes| bytes.is_empty())
    {
        return Ok(c.string_rooted(""));
    }
    let prefix = if dv { "dv" } else { "" };
    let length = intrinsic(c, data, &format!("{prefix}byteLength"), receiver)?;
    let length = c
        .rooted_value(length)
        .and_then(|v| v.as_number())
        .unwrap_or_default() as usize;
    if length == 0 {
        return Ok(c.string_rooted(""));
    }
    let first = index(c, start, 0)?;
    let last = index(c, end, length)?;
    if last <= first {
        return Ok(c.string_rooted(""));
    }
    if last > length {
        return Err(out_of_range(c)?);
    }
    let buffer = intrinsic(c, data, &format!("{prefix}buffer"), receiver)?;
    let offset = intrinsic(c, data, &format!("{prefix}byteOffset"), receiver)?;
    let offset = c
        .rooted_value(offset)
        .and_then(|v| v.as_number())
        .unwrap_or_default() as usize;
    let constructor = get(c, data, "u8")?;
    let offset = c.number((offset + first) as f64);
    let count = c.number((last - first) as f64);
    let view = c.construct_rooted(constructor, constructor, &[buffer, offset, count])?;
    let bytes = c
        .view_bytes_rooted(view)
        .ok_or_else(|| RootedError::host("original byte view missing"))?;
    decode_bytes(c, &bytes, encoding)
}

pub(crate) fn decode_bytes(
    c: &mut Context<'_>,
    bytes: &[u8],
    encoding: &str,
) -> Result<RootId, RootedError> {
    let maximum = Context::MAX_STRING_CODE_UNITS;
    let Some(units) = super::buffer_codec::decode_units_limited(bytes, encoding, maximum) else {
        let e = c.error_rooted(&format!(
            "Cannot create a string longer than 0x{maximum:x} characters"
        ))?;
        let key = c.string_rooted("code");
        let value = c.string_rooted("ERR_STRING_TOO_LONG");
        c.set_property_rooted(e, key, value, e)?;
        return Err(c.throw(e));
    };
    Ok(c.string_units_rooted(&units))
}
pub(crate) fn string_slice(
    c: &mut Context<'_>,
    receiver: RootId,
    args: &[RootId],
) -> Result<RootId, RootedError> {
    let data = c.host_function_data()?;
    let encoding = get(c, data, "encoding")?;
    let encoding = c
        .string_text(encoding)?
        .ok_or_else(|| RootedError::host("slice encoding missing"))?;
    let start = arg(c, args, 0);
    let end = arg(c, args, 1);
    slice(c, data, receiver, start, end, &encoding)
}
pub(crate) fn to_string(
    c: &mut Context<'_>,
    receiver: RootId,
    args: &[RootId],
) -> Result<RootId, RootedError> {
    let data = c.host_function_data()?;
    let zero = c.number(0.0);
    if args.is_empty() {
        let length = get(c, receiver, "length")?;
        return slice(c, data, receiver, zero, length, "utf8");
    }
    let length = intrinsic(c, data, "length", receiver)?;
    let start = arg(c, args, 1);
    let start = if c.greater_equal_rooted(zero, start)? {
        zero
    } else if c.greater_equal_rooted(start, length)? {
        return Ok(c.string_rooted(""));
    } else {
        let n = c.to_number(start)?.trunc();
        c.number(if n.is_nan() || n == 0.0 { 0.0 } else { n })
    };
    let end = arg(c, args, 2);
    let end = if undefined(c, end) || c.greater_than_rooted(end, length)? {
        length
    } else {
        let n = c.to_number(end)?.trunc();
        c.number(if n.is_nan() || n == 0.0 { 0.0 } else { n })
    };
    if c.greater_equal_rooted(start, end)? {
        return Ok(c.string_rooted(""));
    }
    let encoding = arg(c, args, 0);
    let canonical = if undefined(c, encoding) {
        Some("utf8")
    } else {
        let empty = c.string_rooted("");
        let name = c.arithmetic_rooted(NativeArithmetic::Add, encoding, empty)?;
        let name = c.string_text(name)?.unwrap_or_default();
        super::buffer_codec::canonical_encoding(&name)
    };
    let Some(canonical) = canonical else {
        let mut message: Vec<u16> = "Unknown encoding: ".encode_utf16().collect();
        message.extend(c.to_string_code_units(encoding)?);
        let message = c.string_units_rooted(&message);
        let e = c.type_error_rooted("")?;
        let key = c.string_rooted("message");
        c.set_property_rooted(e, key, message, e)?;
        super::node_errors_shared_vm::decorate_type_error(c, e, "ERR_UNKNOWN_ENCODING")?;
        return Err(c.throw(e));
    };
    slice(c, data, receiver, start, end, canonical)
}
