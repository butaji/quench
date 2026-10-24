//! Buffer conversion and raw typed-array copies over Core's original backing stores.
use crate::host::NodeHost;
use quench_runtime::{BuiltinBrand, NativeContext, RootId, RootedError, Value};
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
    c.rooted_value(v).is_some_and(Value::is_undefined)
}
fn getter(c: &mut Context<'_>, p: RootId, n: &str) -> Result<RootId, RootedError> {
    let k = c.string_rooted(n);
    let d = c.own_property_descriptor_rooted(p, k)?;
    get(c, d, "get")
}
fn intrinsic(
    c: &mut Context<'_>,
    data: RootId,
    name: &str,
    value: RootId,
) -> Result<RootId, RootedError> {
    let f = get(c, data, name)?;
    c.call_rooted(f, value, &[])
}
fn allocate(c: &mut Context<'_>, data: RootId, args: &[RootId]) -> Result<RootId, RootedError> {
    let f = get(c, data, "fast")?;
    c.construct_rooted(f, f, args)
}
fn coded(
    c: &mut Context<'_>,
    message: &str,
    code: &'static str,
    range: bool,
) -> Result<RootedError, RootedError> {
    let e = if range {
        c.range_error_rooted(message)?
    } else {
        c.type_error_rooted(message)?
    };
    if range {
        super::node_errors_shared_vm::decorate_range_error(c, e, code)?
    } else {
        super::node_errors_shared_vm::decorate_type_error(c, e, code)?
    };
    Ok(c.throw(e))
}

pub(crate) fn install(
    c: &mut Context<'_>,
    public: RootId,
    fast: RootId,
) -> Result<(), RootedError> {
    let data = c.null_object_rooted()?;
    put(c, data, "fast", fast)?;
    let u8 = c.prototype_rooted(fast)?;
    put(c, data, "u8", u8)?;
    let p = get(c, u8, "prototype")?;
    let typed = c.prototype_rooted(p)?;
    for (n, k) in [
        ("length", "length"),
        ("byteLength", "byteLength"),
        ("offset", "byteOffset"),
        ("buffer", "buffer"),
    ] {
        let f = getter(c, typed, k)?;
        put(c, data, n, f)?;
    }
    for name in ["set", "subarray", "fill"] {
        let method = get(c, typed, name)?;
        put(c, data, name, method)?;
    }
    super::buffer_pool_shared_vm::install(c, data, public)?;
    let global = c.global_root()?;
    let symbol = get(c, global, "Symbol")?;
    let primitive = get(c, symbol, "toPrimitive")?;
    let from_data = c.null_object_rooted()?;
    c.set_prototype_rooted(from_data, data)?;
    put(c, from_data, "primitive", primitive)?;
    for (name, operation) in [
        ("from", "bufferFrom"),
        ("copyBytesFrom", "bufferCopyBytesFrom"),
    ] {
        let operation_data = if name == "from" { from_data } else { data };
        let f = c.host_constructor_with_data(
            crate::host::shared_vm::operation(operation),
            operation_data,
        )?;
        let k = c.string_rooted("name");
        let n = c.string_rooted(name);
        c.define_data_property_rooted(f, k, n, false, false, true)?;
        put(c, public, name, f)?;
        if name == "from" {
            put(c, from_data, "from", f)?;
        }
    }
    super::buffer_pool_shared_vm::install_public_allocators(c, data, public)?;
    Ok(())
}

fn from_string(
    c: &mut Context<'_>,
    data: RootId,
    value: RootId,
    encoding: RootId,
) -> Result<RootId, RootedError> {
    let encoding = c
        .string_text(encoding)?
        .filter(|v| !v.is_empty())
        .unwrap_or_else(|| "utf8".into());
    let Some(encoding) = super::buffer_codec::canonical_encoding(&encoding) else {
        return Err(coded(
            c,
            &format!("Unknown encoding: {encoding}"),
            "ERR_UNKNOWN_ENCODING",
            false,
        )?);
    };
    let units = c
        .string_code_units(value)?
        .ok_or_else(|| RootedError::host("Buffer string conversion lost its string"))?;
    if units.is_empty() {
        return allocate(c, data, &[]);
    }
    let bytes = super::buffer_codec::encode_units(&units, encoding);
    let source = c.uint8_array_rooted(&bytes)?;
    let length = c.number(bytes.len() as f64);
    let (result, pooled) = super::buffer_pool_shared_vm::string(c, data, units.len(), bytes.len())?;
    let set = get(c, data, "set")?;
    let zero = c.number(0.0);
    c.call_rooted(set, result, &[source, zero])?;
    if pooled {
        super::buffer_pool_shared_vm::commit(c, data, length)?;
    }
    Ok(result)
}
fn copy_array_like(
    c: &mut Context<'_>,
    data: RootId,
    source: RootId,
) -> Result<RootId, RootedError> {
    let length = get(c, source, "length")?;
    let zero = c.number(0.0);
    if c.greater_equal_rooted(zero, length)? {
        return allocate(c, data, &[]);
    }
    let (result, pooled) = super::buffer_pool_shared_vm::array(c, data, length)?;
    let set = get(c, data, "set")?;
    c.call_rooted(set, result, &[source, zero])?;
    if pooled {
        super::buffer_pool_shared_vm::commit(c, data, length)?;
    }
    Ok(result)
}
fn from_array_buffer(
    c: &mut Context<'_>,
    data: RootId,
    value: RootId,
    offset: RootId,
    length: RootId,
) -> Result<RootId, RootedError> {
    let offset = if undefined(c, offset) {
        0.0
    } else {
        let n = c.to_number(offset)?;
        if n.is_nan() {
            0.0
        } else {
            n
        }
    };
    let offset = c.number(offset);
    let byte_length = get(c, value, "byteLength")?;
    let maximum = c.subtract_rooted(byte_length, offset)?;
    let zero = c.number(0.0);
    if c.greater_than_rooted(zero, maximum)? {
        return Err(coded(
            c,
            "\"offset\" is outside of buffer bounds",
            "ERR_BUFFER_OUT_OF_BOUNDS",
            true,
        )?);
    }
    let length = if undefined(c, length) {
        length
    } else {
        let n = c.to_number(length)?;
        let n = c.number(n);
        if c.greater_than_rooted(n, zero)? {
            if c.greater_than_rooted(n, maximum)? {
                return Err(coded(
                    c,
                    "\"length\" is outside of buffer bounds",
                    "ERR_BUFFER_OUT_OF_BOUNDS",
                    true,
                )?);
            }
            n
        } else {
            zero
        }
    };
    allocate(c, data, &[value, offset, length])
}

pub(crate) fn from(c: &mut Context<'_>, _: RootId, args: &[RootId]) -> Result<RootId, RootedError> {
    let data = c.host_function_data()?;
    let value = arg(c, args, 0);
    let encoding = arg(c, args, 1);
    let length = arg(c, args, 2);
    if c.string_text(value)?.is_some() {
        return from_string(c, data, value, encoding);
    }
    if c.is_object_rooted(value)? && !c.is_callable_rooted(value)? {
        if c.has_builtin_brand_rooted(value, BuiltinBrand::AnyArrayBuffer)? {
            return from_array_buffer(c, data, value, encoding, length);
        }
        let value_of = get(c, value, "valueOf")?;
        let converted = if c.truthy_rooted(value_of)? {
            let value_of = get(c, value, "valueOf")?;
            if !c.is_callable_rooted(value_of)? {
                let e = c.type_error_rooted("value.valueOf is not a function")?;
                return Err(c.throw(e));
            }
            c.call_rooted(value_of, value, &[])?
        } else {
            value_of
        };
        if c.rooted_value(converted)
            .is_some_and(|v| !v.is_null() && !v.is_undefined())
            && !c.strict_equal_rooted(converted, value)?
            && (c.string_text(converted)?.is_some()
                || (c.is_object_rooted(converted)? && !c.is_callable_rooted(converted)?))
        {
            let from = get(c, data, "from")?;
            let receiver = c.undefined();
            return c.call_rooted(from, receiver, &[converted, encoding, length]);
        }
        let first_length = get(c, value, "length")?;
        let array_like = if !undefined(c, first_length) {
            true
        } else {
            let buffer = get(c, value, "buffer")?;
            c.has_builtin_brand_rooted(buffer, BuiltinBrand::AnyArrayBuffer)?
        };
        if array_like {
            let length = get(c, value, "length")?;
            return if c.rooted_value(length).and_then(Value::as_number).is_none() {
                allocate(c, data, &[])
            } else {
                copy_array_like(c, data, value)
            };
        }
        let kind = get(c, value, "type")?;
        if c.string_text(kind)?.as_deref() == Some("Buffer") {
            let bytes = get(c, value, "data")?;
            if c.is_array_rooted(bytes)? {
                let bytes = get(c, value, "data")?;
                return copy_array_like(c, data, bytes);
            }
        }
        let primitive_key = get(c, data, "primitive")?;
        let primitive = c.get_property_rooted(value, primitive_key)?;
        if c.is_callable_rooted(primitive)? {
            let primitive = c.get_property_rooted(value, primitive_key)?;
            let hint = c.string_rooted("string");
            let converted = c.call_rooted(primitive, value, &[hint])?;
            if c.string_text(converted)?.is_some() {
                return from_string(c, data, converted, encoding);
            }
        }
    }
    Err(super::util_shared_vm::argument_type_error_with_expected(
        c,
        value,
        "first argument",
        "of type string or an instance of Buffer, ArrayBuffer, or Array or an Array-like Object",
    )?)
}

pub(crate) fn copy_bytes(
    c: &mut Context<'_>,
    _: RootId,
    args: &[RootId],
) -> Result<RootId, RootedError> {
    let data = c.host_function_data()?;
    let view = arg(c, args, 0);
    if !c.has_builtin_brand_rooted(view, BuiltinBrand::TypedArray)? {
        return Err(super::util_shared_vm::argument_type_error(
            c,
            view,
            "view",
            "TypedArray",
        )?);
    }
    let length = intrinsic(c, data, "length", view)?;
    let view_length = c
        .rooted_value(length)
        .and_then(Value::as_number)
        .ok_or_else(|| RootedError::host("typed array length is not numeric"))?;
    if view_length == 0.0 {
        return allocate(c, data, &[]);
    }
    let offset = arg(c, args, 1);
    let count = arg(c, args, 2);
    let maximum = c.number(super::util_shared_vm::MAX_SAFE_INTEGER);
    let start = if undefined(c, offset) {
        0.0
    } else {
        super::util_shared_vm::validate_integer(c, offset, "offset", 0.0, maximum)?
    };
    if start >= view_length {
        return allocate(c, data, &[]);
    }
    let end = if undefined(c, count) {
        view_length
    } else {
        (start + super::util_shared_vm::validate_integer(c, count, "length", 0.0, maximum)?)
            .min(view_length)
    };
    if end <= start {
        return allocate(c, data, &[]);
    }
    let byte_length = intrinsic(c, data, "byteLength", view)?;
    let byte_length = c
        .rooted_value(byte_length)
        .and_then(Value::as_number)
        .ok_or_else(|| RootedError::host("typed array byte length is not numeric"))?;
    let width = byte_length / view_length;
    let offset = intrinsic(c, data, "offset", view)?;
    let offset = c
        .rooted_value(offset)
        .and_then(Value::as_number)
        .ok_or_else(|| RootedError::host("typed array byte offset is not numeric"))?;
    let buffer = intrinsic(c, data, "buffer", view)?;
    let byte_start = c.number(offset + start * width);
    let byte_count = c.number((end - start) * width);
    let u8 = get(c, data, "u8")?;
    let bytes = c.construct_rooted(u8, u8, &[buffer, byte_start, byte_count])?;
    copy_array_like(c, data, bytes)
}
