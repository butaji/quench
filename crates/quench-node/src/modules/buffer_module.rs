//! `node:buffer` in Rust: the `Buffer` class (Node's `FastBuffer`, a `Uint8Array` subclass),
//! its statics, its prototype and the module's exports. Byte work goes through the buffer
//! binding (`buffer_binding_shared_vm`) and codecs (`buffer_codec`).

use crate::host::NodeHost;
use crate::modules::buffer_binding_shared_vm as binding;
use crate::modules::buffer_codec as codec;
use quench_runtime::{BuiltinBrand, NativeContext, RootId, RootedError};

type Context<'a> = NativeContext<'a, NodeHost>;

/// `buffer.kMaxLength`: the largest typed array the engine makes.
pub(crate) const MAX_LENGTH: f64 = 9_007_199_254_740_991.0;
/// `buffer.INSPECT_MAX_BYTES`.
const INSPECT_MAX_BYTES: usize = 50;
/// `Buffer.poolSize`.
const POOL_SIZE: f64 = 65_536.0;
/// The widest `readUIntLE`/`writeUIntLE` byte count.
const MAX_VARIABLE_WIDTH: usize = 6;
/// `bidirectionalIndexOf` clamps a numeric byte offset to the int32 range.
const INDEX_OFFSET_MAX: f64 = 2_147_483_647.0;
const INDEX_OFFSET_MIN: f64 = -2_147_483_648.0;
/// `Buffer()`'s deprecation warning and code.
const DEPRECATION: (&str, &str) = (
    "Buffer() is deprecated due to security and usability issues. Please use the Buffer.alloc(), Buffer.allocUnsafe(), or Buffer.from() methods instead.",
    "DEP0005",
);
/// The frames `showFlaggedDeprecation` searches for a node_modules caller.
const DEPRECATION_FRAMES: f64 = 3.0;
/// Bytes per character `toString` and `write` default to.
const DEFAULT_ENCODING: &str = "utf8";

fn arg(c: &mut Context<'_>, args: &[RootId], index: usize) -> RootId {
    match args.get(index) {
        Some(value) => *value,
        None => c.undefined(),
    }
}

fn get(c: &mut Context<'_>, object: RootId, name: &str) -> Result<RootId, RootedError> {
    c.get_named_property_rooted(object, name)
}

fn is_undefined(c: &Context<'_>, value: RootId) -> bool {
    c.rooted_value(value).is_some_and(|value| value.is_undefined())
}

fn number_of(c: &Context<'_>, value: RootId) -> Option<f64> {
    c.rooted_value(value).and_then(|value| value.as_number())
}

fn is_uint8_array(c: &mut Context<'_>, value: RootId) -> Result<bool, RootedError> {
    c.has_builtin_brand_rooted(value, BuiltinBrand::Uint8Array)
}

fn length_of(c: &Context<'_>, view: RootId) -> usize {
    c.with_buffer_bytes_rooted(view, <[u8]>::len).unwrap_or_default()
}

fn coded(c: &mut Context<'_>, error: RootId, code: &str) -> RootedError {
    let value = c.string_rooted(code);
    if let Err(error) = c.assign_named_property_rooted(error, "code", value) {
        return error;
    }
    c.throw(error)
}

/// `ERR_OUT_OF_RANGE`: `The value of "<name>" is out of range. It must be <range>. Received ...`.
fn out_of_range(c: &mut Context<'_>, name: &str, range: &str, value: RootId) -> Result<RootedError, RootedError> {
    let received = super::util_shared_vm::range_error_received(c, value)?;
    let error = c.range_error_rooted(&format!(
        "The value of \"{name}\" is out of range. It must be {range}. Received {received}"
    ))?;
    Ok(coded(c, error, "ERR_OUT_OF_RANGE"))
}

/// `ERR_BUFFER_OUT_OF_BOUNDS`.
fn out_of_bounds(c: &mut Context<'_>, name: Option<&str>) -> Result<RootedError, RootedError> {
    let message = match name {
        Some(name) => format!("\"{name}\" is outside of buffer bounds"),
        None => "Attempt to access memory outside buffer bounds".to_owned(),
    };
    let error = c.range_error_rooted(&message)?;
    Ok(coded(c, error, "ERR_BUFFER_OUT_OF_BOUNDS"))
}

fn unknown_encoding(c: &mut Context<'_>, encoding: RootId) -> Result<RootedError, RootedError> {
    let name = c.to_string(encoding)?;
    let error = c.type_error_rooted(&format!("Unknown encoding: {name}"))?;
    Ok(coded(c, error, "ERR_UNKNOWN_ENCODING"))
}

fn not_a_buffer(c: &mut Context<'_>, value: RootId, name: &str) -> Result<RootedError, RootedError> {
    super::util_shared_vm::argument_type_error_with_expected(c, value, name, "an instance of Buffer or Uint8Array")
}

/// `validateNumber(value, name)`.
fn validate_number(c: &mut Context<'_>, value: RootId, name: &str) -> Result<f64, RootedError> {
    match number_of(c, value) {
        Some(number) => Ok(number),
        None => Err(super::util_shared_vm::argument_type_error(c, value, name, "number")?),
    }
}

/// `validateOffset(value, name, 0, maximum)`.
fn validate_offset(c: &mut Context<'_>, value: RootId, name: &str, maximum: f64) -> Result<usize, RootedError> {
    let maximum = c.number(maximum);
    Ok(super::util_shared_vm::validate_integer(c, value, name, 0.0, maximum)? as usize)
}

/// `normalizeEncoding`: a canonical encoding name, `utf8` for undefined or empty.
fn encoding_of(c: &mut Context<'_>, encoding: RootId) -> Result<Option<&'static str>, RootedError> {
    if is_undefined(c, encoding) {
        return Ok(Some(DEFAULT_ENCODING));
    }
    match c.string_text(encoding)? {
        Some(name) if name.is_empty() => Ok(Some(DEFAULT_ENCODING)),
        Some(name) => Ok(codec::canonical_encoding(&name)),
        None => Ok(None),
    }
}

/// The encoding `encoding` names, or `ERR_UNKNOWN_ENCODING`.
fn required_encoding(c: &mut Context<'_>, encoding: RootId) -> Result<&'static str, RootedError> {
    match encoding_of(c, encoding)? {
        Some(name) => Ok(name),
        None => Err(unknown_encoding(c, encoding)?),
    }
}

/// `toInteger(n, default)`.
fn to_integer(c: &mut Context<'_>, value: RootId, default: f64) -> Result<f64, RootedError> {
    let number = c.to_number(value)?;
    if !number.is_nan() && (-MAX_LENGTH..=MAX_LENGTH).contains(&number) {
        Ok(number.floor())
    } else {
        Ok(default)
    }
}

/// `adjustOffset(offset, length)`.
fn adjust_offset(c: &mut Context<'_>, offset: RootId, length: usize) -> Result<usize, RootedError> {
    let offset = c.to_number(offset)?.trunc();
    let length = length as f64;
    Ok(if offset == 0.0 || offset.is_nan() {
        0.0
    } else if offset < 0.0 {
        (offset + length).max(0.0)
    } else {
        offset.min(length)
    } as usize)
}

/// The canonical `Buffer` constructor and its prototype.
pub(crate) struct Classes {
    pub(crate) buffer: RootId,
    pub(crate) prototype: RootId,
}

fn classes(c: &mut Context<'_>) -> Result<Classes, RootedError> {
    let buffer = super::buffer_shared_vm::fast_buffer(c)?;
    let prototype = get(c, buffer, "prototype")?;
    Ok(Classes { buffer, prototype })
}

/// A Buffer view of `args` as `new Uint8Array(...args)` makes it, with `Buffer.prototype`.
fn construct_view(c: &mut Context<'_>, args: &[RootId]) -> Result<RootId, RootedError> {
    let Classes { buffer, .. } = classes(c)?;
    let global = c.global_root()?;
    let uint8 = get(c, global, "Uint8Array")?;
    c.construct_rooted(uint8, buffer, args)
}

/// A zero-filled Buffer of `size` bytes.
fn zeroed(c: &mut Context<'_>, size: usize) -> Result<RootId, RootedError> {
    super::buffer_shared_vm::from_vec(c, vec![0; size])
}

fn from_bytes(c: &mut Context<'_>, bytes: Vec<u8>) -> Result<RootId, RootedError> {
    super::buffer_shared_vm::from_vec(c, bytes)
}

/// `Buffer`: `new Buffer(...)` and `Buffer(...)` both make a Buffer; a typed-array style
/// argument list (a length, or an ArrayBuffer with offset and length) makes it as
/// `Uint8Array` does, anything else as `Buffer.from` does.
pub(crate) fn constructor(c: &mut Context<'_>, _: RootId, args: &[RootId]) -> Result<RootId, RootedError> {
    show_flagged_deprecation(c)?;
    let first = arg(c, args, 0);
    if number_of(c, first).is_some() {
        let second = arg(c, args, 1);
        if c.string_text(second)?.is_some() {
            return Err(super::util_shared_vm::argument_type_error(c, first, "string", "string")?);
        }
        return alloc(c, first, &[first]);
    }
    if c.has_builtin_brand_rooted(first, BuiltinBrand::AnyArrayBuffer)? && c.new_target_rooted().is_some() {
        return construct_view(c, args);
    }
    let (second, third) = (arg(c, args, 1), arg(c, args, 2));
    from(c, first, second, third)
}

/// `showFlaggedDeprecation`: `Buffer()` warns once (DEP0005), unless its caller runs from
/// node_modules without `--pending-deprecation`.
fn show_flagged_deprecation(c: &mut Context<'_>) -> Result<(), RootedError> {
    let pending = c.host_mut().shared_state().borrow().cli_options.enabled("--pending-deprecation");
    if !pending {
        let frames = c.number(DEPRECATION_FRAMES);
        let undefined = c.undefined();
        let inside = super::util_shared_vm::is_inside_node_modules(c, undefined, &[frames])?;
        if c.truthy_rooted(inside)? {
            return Ok(());
        }
    }
    let undefined = c.undefined();
    super::fs_shared_vm::emit_deprecation(c, DEPRECATION.0, DEPRECATION.1, undefined)
}

/// `new FastBuffer(bufferOrLength, byteOffset, length)`: a Buffer made as `Uint8Array` makes
/// a view.
pub(crate) fn fast_constructor(c: &mut Context<'_>, _: RootId, args: &[RootId]) -> Result<RootId, RootedError> {
    construct_view(c, args)
}

/// `Buffer.from(value[, encodingOrOffset[, length]])`.
pub(crate) fn static_from(c: &mut Context<'_>, _: RootId, args: &[RootId]) -> Result<RootId, RootedError> {
    let (value, second, third) = (arg(c, args, 0), arg(c, args, 1), arg(c, args, 2));
    from(c, value, second, third)
}

fn from(c: &mut Context<'_>, value: RootId, second: RootId, third: RootId) -> Result<RootId, RootedError> {
    if c.string_text(value)?.is_some() {
        return from_string(c, value, second);
    }
    if c.is_object_rooted(value)? {
        if c.has_builtin_brand_rooted(value, BuiltinBrand::AnyArrayBuffer)? {
            return from_array_buffer(c, value, second, third);
        }
        let value_of = get(c, value, "valueOf")?;
        if c.is_callable_rooted(value_of)? {
            let primitive = c.call_rooted(value_of, value, &[])?;
            let replaced = !c.rooted_value(primitive).is_some_and(|value| value.is_undefined() || value.is_null())
                && !c.strict_equal_rooted(primitive, value)?
                && (c.string_text(primitive)?.is_some() || c.is_object_rooted(primitive)?);
            if replaced {
                return from(c, primitive, second, third);
            }
        }
        if let Some(buffer) = from_object(c, value)? {
            return Ok(buffer);
        }
        let global = c.global_root()?;
        let symbol = get(c, global, "Symbol")?;
        let to_primitive = get(c, symbol, "toPrimitive")?;
        let method = c.get_property_rooted(value, to_primitive)?;
        if c.is_callable_rooted(method)? {
            let hint = c.string_rooted("string");
            let primitive = c.call_rooted(method, value, &[hint])?;
            if c.string_text(primitive)?.is_some() {
                return from_string(c, primitive, second);
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

/// `fromString(string, encoding)`.
fn from_string(c: &mut Context<'_>, string: RootId, encoding: RootId) -> Result<RootId, RootedError> {
    let units = c.string_code_units(string)?.unwrap_or_default();
    let encoding = if c.string_text(encoding)?.is_some_and(|name| !name.is_empty()) {
        required_encoding(c, encoding)?
    } else {
        DEFAULT_ENCODING
    };
    from_bytes(c, codec::encode_units(&units, encoding))
}

/// `fromArrayBuffer(buffer, byteOffset, length)`: a view sharing the buffer's memory.
fn from_array_buffer(c: &mut Context<'_>, buffer: RootId, offset: RootId, length: RootId) -> Result<RootId, RootedError> {
    let byte_length = length_of(c, buffer) as f64;
    let offset = if is_undefined(c, offset) {
        0.0
    } else {
        let offset = c.to_number(offset)?;
        if offset.is_nan() { 0.0 } else { offset }
    };
    let maximum = byte_length - offset;
    if maximum < 0.0 {
        return Err(out_of_bounds(c, Some("offset"))?);
    }
    let length = if is_undefined(c, length) {
        maximum
    } else {
        let length = c.to_number(length)?;
        if length > 0.0 {
            if length > maximum {
                return Err(out_of_bounds(c, Some("length"))?);
            }
            length
        } else {
            0.0
        }
    };
    let offset = c.number(offset);
    let length = c.number(length);
    construct_view(c, &[buffer, offset, length])
}

/// `fromObject(obj)`: an array-like or a `{ type: 'Buffer', data }` record, copied.
fn from_object(c: &mut Context<'_>, value: RootId) -> Result<Option<RootId>, RootedError> {
    if is_uint8_array(c, value)? {
        let bytes = c.view_bytes_rooted(value).unwrap_or_default();
        return from_bytes(c, bytes).map(Some);
    }
    let length = get(c, value, "length")?;
    let backing = get(c, value, "buffer")?;
    if !is_undefined(c, length) || c.has_builtin_brand_rooted(backing, BuiltinBrand::AnyArrayBuffer)? {
        if number_of(c, length).is_none() {
            return zeroed(c, 0).map(Some);
        }
        return from_array_like(c, value).map(Some);
    }
    let kind = get(c, value, "type")?;
    let data = get(c, value, "data")?;
    if c.string_text(kind)?.as_deref() == Some("Buffer") && c.is_array_rooted(data)? {
        return from_array_like(c, data).map(Some);
    }
    Ok(None)
}

/// `fromArrayLike(obj)`: each element converted as a `Uint8Array` stores it.
fn from_array_like(c: &mut Context<'_>, value: RootId) -> Result<RootId, RootedError> {
    let values = c.array_like_values_rooted(value)?;
    let array = c.array_rooted(&values)?;
    construct_view(c, &[array])
}

/// `Buffer.alloc(size[, fill[, encoding]])`.
pub(crate) fn alloc(c: &mut Context<'_>, _: RootId, args: &[RootId]) -> Result<RootId, RootedError> {
    let size = arg(c, args, 0);
    let size = validate_size(c, size)?;
    let buffer = zeroed(c, size)?;
    let fill_value = arg(c, args, 1);
    let zero = number_of(c, fill_value) == Some(0.0);
    if !is_undefined(c, fill_value) && !zero && size > 0 {
        let start = c.undefined();
        let encoding = arg(c, args, 2);
        fill_buffer(c, buffer, fill_value, start, start, encoding, false)?;
    }
    Ok(buffer)
}

/// `validateNumber(size, 'size', 0, kMaxLength)`.
fn validate_size(c: &mut Context<'_>, size: RootId) -> Result<usize, RootedError> {
    let number = validate_number(c, size, "size")?;
    if !(0.0..=MAX_LENGTH).contains(&number) {
        return Err(out_of_range(c, "size", &format!(">= 0 && <= {MAX_LENGTH}"), size)?);
    }
    Ok(number as usize)
}

/// `Buffer.allocUnsafe(size)` and `Buffer.allocUnsafeSlow(size)`: their contents are
/// unspecified; these are zero-filled.
pub(crate) fn alloc_unsafe(c: &mut Context<'_>, _: RootId, args: &[RootId]) -> Result<RootId, RootedError> {
    let size = arg(c, args, 0);
    let size = validate_size(c, size)?;
    zeroed(c, size)
}

/// `Buffer.isBuffer(value)`: whether `Buffer.prototype` is on its prototype chain.
pub(crate) fn is_buffer(c: &mut Context<'_>, _: RootId, args: &[RootId]) -> Result<RootId, RootedError> {
    let value = arg(c, args, 0);
    let result = is_buffer_value(c, value)?;
    Ok(c.boolean(result))
}

fn is_buffer_value(c: &mut Context<'_>, value: RootId) -> Result<bool, RootedError> {
    if !c.is_object_rooted(value)? {
        return Ok(false);
    }
    let Classes { prototype, .. } = classes(c)?;
    let mut current = c.prototype_rooted(value)?;
    while c.is_object_rooted(current)? {
        if c.strict_equal_rooted(current, prototype)? {
            return Ok(true);
        }
        current = c.prototype_rooted(current)?;
    }
    Ok(false)
}

/// `Buffer.isEncoding(encoding)`.
pub(crate) fn is_encoding(c: &mut Context<'_>, _: RootId, args: &[RootId]) -> Result<RootId, RootedError> {
    let encoding = arg(c, args, 0);
    let known = c.string_text(encoding)?.is_some_and(|name| !name.is_empty() && codec::canonical_encoding(&name).is_some());
    Ok(c.boolean(known))
}

/// `base64ByteLength(str, bytes)`.
fn base64_byte_length(units: &[u16]) -> usize {
    const PADDING: u16 = b'=' as u16;
    let mut bytes = units.len();
    if bytes > 0 && units[bytes - 1] == PADDING {
        bytes -= 1;
    }
    if bytes > 1 && units[bytes - 1] == PADDING {
        bytes -= 1;
    }
    (bytes * 3) >> 2
}

/// `Buffer.byteLength(string[, encoding])`.
pub(crate) fn byte_length(c: &mut Context<'_>, _: RootId, args: &[RootId]) -> Result<RootId, RootedError> {
    let string = arg(c, args, 0);
    let Some(units) = c.string_code_units(string)? else {
        if c.has_builtin_brand_rooted(string, BuiltinBrand::ArrayBufferView)?
            || c.has_builtin_brand_rooted(string, BuiltinBrand::AnyArrayBuffer)?
        {
            let length = length_of(c, string);
            return Ok(c.number(length as f64));
        }
        return Err(super::util_shared_vm::argument_type_error_with_expected(
            c,
            string,
            "string",
            "of type string or an instance of Buffer or ArrayBuffer",
        )?);
    };
    let encoding = arg(c, args, 1);
    let length = match encoding_of(c, encoding)?.unwrap_or(DEFAULT_ENCODING) {
        "utf16le" => units.len() * 2,
        "latin1" | "ascii" => units.len(),
        "hex" => units.len() >> 1,
        "base64" | "base64url" => base64_byte_length(&units),
        _ => c.string_utf8_length(string)?.unwrap_or_default(),
    };
    Ok(c.number(length as f64))
}

/// `Buffer.compare(buf1, buf2)`.
pub(crate) fn static_compare(c: &mut Context<'_>, _: RootId, args: &[RootId]) -> Result<RootId, RootedError> {
    let (first, second) = (arg(c, args, 0), arg(c, args, 1));
    if !is_uint8_array(c, first)? {
        return Err(not_a_buffer(c, first, "buf1")?);
    }
    if !is_uint8_array(c, second)? {
        return Err(not_a_buffer(c, second, "buf2")?);
    }
    let missing = c.undefined();
    binding::compare(c, missing, &[first, second])
}

/// `Buffer.concat(list[, totalLength])`.
pub(crate) fn concat(c: &mut Context<'_>, _: RootId, args: &[RootId]) -> Result<RootId, RootedError> {
    let list = arg(c, args, 0);
    if !c.is_array_rooted(list)? {
        return Err(super::util_shared_vm::argument_type_error_with_expected(c, list, "list", "an instance of Array")?);
    }
    let items = c.array_like_values_rooted(list)?;
    if items.is_empty() {
        return zeroed(c, 0);
    }
    let mut parts = Vec::with_capacity(items.len());
    for (index, item) in items.iter().enumerate() {
        if !is_uint8_array(c, *item)? {
            return Err(not_a_buffer(c, *item, &format!("list[{index}]"))?);
        }
        parts.push(c.view_bytes_rooted(*item).unwrap_or_default());
    }
    let total = arg(c, args, 1);
    let total = if is_undefined(c, total) {
        parts.iter().map(Vec::len).sum()
    } else {
        validate_offset(c, total, "length", MAX_LENGTH)?
    };
    let mut bytes = Vec::with_capacity(total);
    for part in parts {
        let room = total - bytes.len();
        bytes.extend_from_slice(&part[..part.len().min(room)]);
    }
    bytes.resize(total, 0);
    from_bytes(c, bytes)
}

/// `Buffer.of(...items)`.
pub(crate) fn of(c: &mut Context<'_>, _: RootId, args: &[RootId]) -> Result<RootId, RootedError> {
    let array = c.array_rooted(args)?;
    construct_view(c, &[array])
}

/// `Buffer.copyBytesFrom(view[, offset[, length]])`: a copy of a typed array's elements'
/// bytes.
pub(crate) fn copy_bytes_from(c: &mut Context<'_>, _: RootId, args: &[RootId]) -> Result<RootId, RootedError> {
    let view = arg(c, args, 0);
    if !c.has_builtin_brand_rooted(view, BuiltinBrand::TypedArray)? {
        return Err(super::util_shared_vm::argument_type_error_with_expected(c, view, "view", "an instance of TypedArray")?);
    }
    let elements = get(c, view, "length")?;
    let elements = number_of(c, elements).unwrap_or_default() as usize;
    let bytes = c.view_bytes_rooted(view).unwrap_or_default();
    let width = bytes.len().checked_div(elements).unwrap_or(1).max(1);
    let offset = arg(c, args, 1);
    let start = if is_undefined(c, offset) { 0 } else { validate_offset(c, offset, "offset", MAX_LENGTH)? };
    if start >= elements {
        return zeroed(c, 0);
    }
    let length = arg(c, args, 2);
    let count = if is_undefined(c, length) { elements - start } else { validate_offset(c, length, "length", MAX_LENGTH)?.min(elements - start) };
    from_bytes(c, bytes[start * width..(start + count) * width].to_vec())
}

/// `buf.toString([encoding[, start[, end]]])`.
pub(crate) fn to_string(c: &mut Context<'_>, this: RootId, args: &[RootId]) -> Result<RootId, RootedError> {
    let bytes = c.view_bytes_rooted(this).unwrap_or_default();
    let length = bytes.len();
    if args.is_empty() {
        return binding::decode_bytes(c, &bytes, DEFAULT_ENCODING);
    }
    let start = arg(c, args, 1);
    let start = c.to_number(start)?;
    let start = if start <= 0.0 {
        0
    } else if start >= length as f64 {
        return Ok(c.string_rooted(""));
    } else if start.is_nan() {
        0
    } else {
        start.trunc() as usize
    };
    let end = arg(c, args, 2);
    let end = if is_undefined(c, end) {
        length
    } else {
        let end = c.to_number(end)?;
        if end > length as f64 {
            length
        } else if end.is_nan() || end <= 0.0 {
            0
        } else {
            end.trunc() as usize
        }
    };
    if end <= start {
        return Ok(c.string_rooted(""));
    }
    let encoding = arg(c, args, 0);
    if is_undefined(c, encoding) {
        return binding::decode_bytes(c, &bytes[start..end], DEFAULT_ENCODING);
    }
    // `getEncodingOps` coerces the encoding (`encoding += ''`); no name is no encoding.
    let name = c.to_string(encoding)?;
    match codec::canonical_encoding(&name) {
        Some(encoding) => binding::decode_bytes(c, &bytes[start..end], encoding),
        None => {
            let name = c.string_rooted(&name);
            Err(unknown_encoding(c, name)?)
        }
    }
}

/// `buf.equals(otherBuffer)`.
pub(crate) fn equals(c: &mut Context<'_>, this: RootId, args: &[RootId]) -> Result<RootId, RootedError> {
    let other = arg(c, args, 0);
    if !is_uint8_array(c, other)? {
        return Err(not_a_buffer(c, other, "otherBuffer")?);
    }
    let same = c.view_bytes_rooted(this) == c.view_bytes_rooted(other);
    Ok(c.boolean(same))
}

/// `buf.compare(target[, targetStart[, targetEnd[, sourceStart[, sourceEnd]]]])`.
pub(crate) fn compare(c: &mut Context<'_>, this: RootId, args: &[RootId]) -> Result<RootId, RootedError> {
    let target = arg(c, args, 0);
    if !is_uint8_array(c, target)? {
        return Err(not_a_buffer(c, target, "target")?);
    }
    let missing = c.undefined();
    if args.len() <= 1 {
        return binding::compare(c, missing, &[this, target]);
    }
    let target_length = length_of(c, target);
    let source_length = length_of(c, this);
    let bound = |c: &mut Context<'_>, index: usize, name: &str, default: usize, maximum: usize| -> Result<usize, RootedError> {
        let value = arg(c, args, index);
        if is_undefined(c, value) { Ok(default) } else { validate_offset(c, value, name, maximum as f64) }
    };
    let target_start = bound(c, 1, "targetStart", 0, MAX_LENGTH as usize)?;
    let target_end = bound(c, 2, "targetEnd", target_length, target_length)?;
    let source_start = bound(c, 3, "sourceStart", 0, MAX_LENGTH as usize)?;
    let source_end = bound(c, 4, "sourceEnd", source_length, source_length)?;
    if source_start >= source_end {
        return Ok(c.number(if target_start >= target_end { 0.0 } else { -1.0 }));
    }
    if target_start >= target_end {
        return Ok(c.number(1.0));
    }
    let values = [target_start, source_start, target_end, source_end].map(|value| c.number(value as f64));
    binding::compare_offset(c, missing, &[this, target, values[0], values[1], values[2], values[3]])
}

/// `buf.copy(target[, targetStart[, sourceStart[, sourceEnd]]])`.
pub(crate) fn copy(c: &mut Context<'_>, this: RootId, args: &[RootId]) -> Result<RootId, RootedError> {
    let target = arg(c, args, 0);
    if !is_uint8_array(c, this)? {
        return Err(not_a_buffer(c, this, "source")?);
    }
    if !is_uint8_array(c, target)? {
        return Err(not_a_buffer(c, target, "target")?);
    }
    let source_length = length_of(c, this);
    let target_length = length_of(c, target);
    let target_start = arg(c, args, 1);
    let target_start = if is_undefined(c, target_start) {
        0.0
    } else {
        let value = to_integer(c, target_start, 0.0)?;
        if value < 0.0 {
            return Err(out_of_range(c, "targetStart", ">= 0", target_start)?);
        }
        value
    };
    let source_start = arg(c, args, 2);
    let source_start = if is_undefined(c, source_start) {
        0.0
    } else {
        let value = to_integer(c, source_start, 0.0)?;
        if value < 0.0 || value > source_length as f64 {
            return Err(out_of_range(c, "sourceStart", &format!(">= 0 && <= {source_length}"), source_start)?);
        }
        value
    };
    let source_end = arg(c, args, 3);
    let mut source_end = if is_undefined(c, source_end) {
        source_length as f64
    } else {
        let value = to_integer(c, source_end, 0.0)?;
        if value < 0.0 {
            return Err(out_of_range(c, "sourceEnd", ">= 0", source_end)?);
        }
        value
    };
    if target_start >= target_length as f64 || source_start >= source_end {
        return Ok(c.number(0.0));
    }
    let room = target_length as f64 - target_start;
    if source_end - source_start > room {
        source_end = source_start + room;
    }
    let count = (source_end - source_start).min(source_length as f64 - source_start);
    if count <= 0.0 {
        return Ok(c.number(0.0));
    }
    let missing = c.undefined();
    let values = [target_start, source_start, count].map(|value| c.number(value));
    binding::copy(c, missing, &[this, target, values[0], values[1], values[2]])
}

/// `buf.fill(value[, offset[, end]][, encoding])`.
pub(crate) fn fill(c: &mut Context<'_>, this: RootId, args: &[RootId]) -> Result<RootId, RootedError> {
    let (value, offset, end, encoding) = (arg(c, args, 0), arg(c, args, 1), arg(c, args, 2), arg(c, args, 3));
    fill_buffer(c, this, value, offset, end, encoding, true)
}

/// `_fill(buf, value, offset, end, encoding)`.
fn fill_buffer(
    c: &mut Context<'_>,
    buffer: RootId,
    value: RootId,
    mut offset: RootId,
    mut end: RootId,
    mut encoding: RootId,
    checked: bool,
) -> Result<RootId, RootedError> {
    let length = length_of(c, buffer);
    let mut value = value;
    let mut text_encoding = None;
    if let Some(units) = c.string_code_units(value)? {
        if is_undefined(c, offset) || c.string_text(offset)?.is_some() {
            encoding = offset;
            offset = c.number(0.0);
            end = c.number(length as f64);
        } else if c.string_text(end)?.is_some() {
            encoding = end;
            end = c.number(length as f64);
        }
        let name = match encoding_of(c, encoding)? {
            Some(name) => name,
            None => {
                if c.string_text(encoding)?.is_none() {
                    return Err(super::util_shared_vm::argument_type_error(c, encoding, "encoding", "string")?);
                }
                return Err(unknown_encoding(c, encoding)?);
            }
        };
        match (units.len(), name) {
            (0, _) => value = c.number(0.0),
            (1, "utf8") if units[0] < 0x80 => value = c.number(f64::from(units[0])),
            (1, "latin1") => value = c.number(f64::from(units[0])),
            _ => {}
        }
        text_encoding = Some(name);
    }
    let (start, stop) = if is_undefined(c, offset) {
        (0, length)
    } else {
        let start = validate_offset(c, offset, "offset", MAX_LENGTH)?;
        let stop = if is_undefined(c, end) { length } else { validate_offset(c, end, "end", length as f64)? };
        if start >= stop {
            return Ok(buffer);
        }
        (start, stop)
    };
    if checked && stop > length {
        return Err(out_of_bounds(c, None)?);
    }
    if let Some(number) = number_of(c, value) {
        if start > stop || stop > length {
            return Err(out_of_bounds(c, None)?);
        }
        let byte = (number as i64 & 0xff) as u8;
        c.with_buffer_bytes_mut_rooted(buffer, |bytes| bytes[start..stop].fill(byte));
        return Ok(buffer);
    }
    let missing = c.undefined();
    let start_value = c.number(start as f64);
    let stop_value = c.number(stop as f64);
    let encoding_value = c.string_rooted(text_encoding.unwrap_or(DEFAULT_ENCODING));
    let result = binding::fill(c, missing, &[buffer, value, start_value, stop_value, encoding_value])?;
    match number_of(c, result) {
        Some(FILL_INVALID_VALUE) => Err(super::util_shared_vm::argument_value_error(c, value, "value", "is invalid")?),
        Some(code) if code < 0.0 => Err(out_of_bounds(c, None)?),
        _ => Ok(buffer),
    }
}

/// `buf.write(string[, offset[, length]][, encoding])`.
pub(crate) fn write(c: &mut Context<'_>, this: RootId, args: &[RootId]) -> Result<RootId, RootedError> {
    let string = arg(c, args, 0);
    let Some(units) = c.string_code_units(string)? else {
        return Err(super::util_shared_vm::argument_type_error(c, string, "string", "string")?);
    };
    let length = length_of(c, this);
    let (offset, count, encoding) = (arg(c, args, 1), arg(c, args, 2), arg(c, args, 3));
    let (start, count, encoding) = if is_undefined(c, offset) {
        (0, length, DEFAULT_ENCODING)
    } else if is_undefined(c, count) && c.string_text(offset)?.is_some() {
        (0, length, required_encoding(c, offset)?)
    } else {
        let start = validate_offset(c, offset, "offset", length as f64)?;
        let remaining = length - start;
        if is_undefined(c, count) {
            (start, remaining, required_encoding(c, encoding)?)
        } else if c.string_text(count)?.is_some() {
            (start, remaining, required_encoding(c, count)?)
        } else {
            let count = validate_offset(c, count, "length", length as f64)?.min(remaining);
            (start, count, required_encoding(c, encoding)?)
        }
    };
    let written = write_units(c, this, &units, start, count, encoding);
    Ok(c.number(written as f64))
}

/// Writes the bytes `units` encode to, as many whole characters as fit in `count` bytes at
/// `start`; the bytes written.
fn write_units(c: &mut Context<'_>, view: RootId, units: &[u16], start: usize, count: usize, encoding: &str) -> usize {
    let encoded = codec::encode_units(units, encoding);
    let written = binding::writable_prefix(&encoded, encoding, count);
    c.with_buffer_bytes_mut_rooted(view, |bytes| {
        let end = (start + written).min(bytes.len());
        bytes[start..end].copy_from_slice(&encoded[..end - start]);
        end - start
    })
    .unwrap_or_default()
}

/// `buf.<encoding>Write(string[, offset[, length]])`, as `internal/buffer.js` defines them.
pub(crate) fn encoding_write(c: &mut Context<'_>, this: RootId, args: &[RootId]) -> Result<RootId, RootedError> {
    let data = c.host_function_data()?;
    let encoding = c.string_text(data)?.unwrap_or_default();
    let encoding = codec::canonical_encoding(&encoding).unwrap_or(DEFAULT_ENCODING);
    let string = arg(c, args, 0);
    let Some(units) = c.string_code_units(string)? else {
        return Err(super::util_shared_vm::argument_type_error(c, string, "string", "string")?);
    };
    let length = length_of(c, this);
    let offset = arg(c, args, 1);
    let start = if is_undefined(c, offset) { 0.0 } else { c.to_number(offset)? };
    if !(0.0..=length as f64).contains(&start) {
        return Err(out_of_bounds(c, Some("offset"))?);
    }
    let start = start as usize;
    let count = arg(c, args, 2);
    let count = if is_undefined(c, count) { (length - start) as f64 } else { c.to_number(count)? };
    if !(0.0..=(length - start) as f64).contains(&count) {
        return Err(out_of_bounds(c, Some("length"))?);
    }
    let written = write_units(c, this, &units, start, count as usize, encoding);
    Ok(c.number(written as f64))
}

/// `buf.<encoding>Slice(start, end)`.
pub(crate) fn encoding_slice(c: &mut Context<'_>, this: RootId, args: &[RootId]) -> Result<RootId, RootedError> {
    let data = c.host_function_data()?;
    let encoding = c.string_text(data)?.unwrap_or_default();
    let encoding = codec::canonical_encoding(&encoding).unwrap_or(DEFAULT_ENCODING);
    let bytes = c.view_bytes_rooted(this).unwrap_or_default();
    let start = arg(c, args, 0);
    let start = if is_undefined(c, start) { 0 } else { to_integer(c, start, 0.0)?.max(0.0) as usize };
    let end = arg(c, args, 1);
    let end = if is_undefined(c, end) { bytes.len() } else { to_integer(c, end, 0.0)?.max(0.0) as usize };
    if start > bytes.len() || end > bytes.len() {
        let error = c.range_error_rooted("Index out of range")?;
        return Err(coded(c, error, "ERR_OUT_OF_RANGE"));
    }
    binding::decode_bytes(c, &bytes[start..end.max(start)], encoding)
}

/// `buf.toJSON()`.
pub(crate) fn to_json(c: &mut Context<'_>, this: RootId, _: &[RootId]) -> Result<RootId, RootedError> {
    let bytes = c.view_bytes_rooted(this).unwrap_or_default();
    let values: Vec<RootId> = bytes.iter().map(|byte| c.number(f64::from(*byte))).collect();
    let data = c.array_rooted(&values)?;
    let record = c.object_rooted()?;
    let kind = c.string_rooted("Buffer");
    c.assign_named_property_rooted(record, "type", kind)?;
    c.assign_named_property_rooted(record, "data", data)?;
    Ok(record)
}

/// `buf.slice([start[, end]])`: a view of the same memory.
pub(crate) fn slice(c: &mut Context<'_>, this: RootId, args: &[RootId]) -> Result<RootId, RootedError> {
    let length = length_of(c, this);
    let start = arg(c, args, 0);
    let start = adjust_offset(c, start, length)?;
    let end = arg(c, args, 1);
    let end = if is_undefined(c, end) { length } else { adjust_offset(c, end, length)? };
    let count = end.saturating_sub(start);
    let backing = get(c, this, "buffer")?;
    let base = get(c, this, "byteOffset")?;
    let base = number_of(c, base).unwrap_or_default() as usize;
    let offset = c.number((base + start) as f64);
    let count = c.number(count as f64);
    construct_view(c, &[backing, offset, count])
}

/// `buf.indexOf`, `buf.lastIndexOf` and `buf.includes` (`bidirectionalIndexOf`); the data
/// says which.
pub(crate) fn index_of(c: &mut Context<'_>, this: RootId, args: &[RootId]) -> Result<RootId, RootedError> {
    let data = c.host_function_data()?;
    let mode = number_of(c, data).unwrap_or_default();
    let (forward, includes) = match mode as u8 {
        SEARCH_LAST => (false, false),
        SEARCH_INCLUDES => (true, true),
        _ => (true, false),
    };
    let found = search(c, this, args, forward)?;
    Ok(if includes { c.boolean(found != -1.0) } else { c.number(found) })
}

/// The binding's `fill` result for a value it cannot fill with.
const FILL_INVALID_VALUE: f64 = -1.0;

const SEARCH_FIRST: u8 = 0;
const SEARCH_LAST: u8 = 1;
const SEARCH_INCLUDES: u8 = 2;

fn search(c: &mut Context<'_>, buffer: RootId, args: &[RootId], forward: bool) -> Result<f64, RootedError> {
    let value = arg(c, args, 0);
    let mut offset = arg(c, args, 1);
    let mut encoding = arg(c, args, 2);
    if c.string_text(offset)?.is_some() {
        encoding = offset;
        offset = c.undefined();
    }
    let mut byte_offset = if is_undefined(c, offset) { f64::NAN } else { c.to_number(offset)? };
    byte_offset = byte_offset.clamp(INDEX_OFFSET_MIN, INDEX_OFFSET_MAX);
    if byte_offset.is_nan() {
        byte_offset = if forward { 0.0 } else { length_of(c, buffer) as f64 };
    }
    let missing = c.undefined();
    let offset = c.number(byte_offset);
    let direction = c.boolean(forward);
    let end = c.number(length_of(c, buffer) as f64);
    if let Some(number) = number_of(c, value) {
        let byte = c.number(f64::from((number as i64 as u32) & 0xff));
        let result = binding::index_of_number(c, missing, &[buffer, byte, offset, direction, end])?;
        return Ok(number_of(c, result).unwrap_or(-1.0));
    }
    let name = encoding_of(c, encoding)?;
    let id = |name: &str| super::string_decoder_codec::ENCODINGS.iter().position(|known| *known == name).unwrap_or(1);
    if c.string_text(value)?.is_some() {
        let Some(name) = name else {
            return Err(unknown_encoding(c, encoding)?);
        };
        let encoding = c.number(id(name) as f64);
        let result = binding::index_of_string(c, missing, &[buffer, value, offset, encoding, direction, end])?;
        return Ok(number_of(c, result).unwrap_or(-1.0));
    }
    if is_uint8_array(c, value)? {
        let encoding = c.number(id(name.unwrap_or(DEFAULT_ENCODING)) as f64);
        let result = binding::index_of_buffer(c, missing, &[buffer, value, offset, encoding, direction, end])?;
        return Ok(number_of(c, result).unwrap_or(-1.0));
    }
    Err(super::util_shared_vm::argument_type_error_with_expected(
        c,
        value,
        "value",
        "one of type number or string or an instance of Buffer or Uint8Array",
    )?)
}

/// `buf.swap16()`, `swap32()`, `swap64()`; the data is the unit width in bytes.
pub(crate) fn swap(c: &mut Context<'_>, this: RootId, _: &[RootId]) -> Result<RootId, RootedError> {
    let data = c.host_function_data()?;
    let width = number_of(c, data).unwrap_or(2.0) as usize;
    let length = length_of(c, this);
    if !length.is_multiple_of(width) {
        let error = c.range_error_rooted(&format!("Buffer size must be a multiple of {}-bits", width * 8))?;
        return Err(coded(c, error, "ERR_INVALID_BUFFER_SIZE"));
    }
    c.with_buffer_bytes_mut_rooted(this, |bytes| bytes.chunks_exact_mut(width).for_each(<[u8]>::reverse));
    Ok(this)
}

/// `buf[util.inspect.custom]()`: `<Buffer 01 02 ... N more bytes>`.
pub(crate) fn inspect(c: &mut Context<'_>, this: RootId, _: &[RootId]) -> Result<RootId, RootedError> {
    let bytes = c.view_bytes_rooted(this).unwrap_or_default();
    let shown = bytes.len().min(INSPECT_MAX_BYTES);
    let mut text = bytes[..shown].iter().map(|byte| format!("{byte:02x}")).collect::<Vec<_>>().join(" ");
    let rest = bytes.len() - shown;
    if rest > 0 {
        text.push_str(&format!(" ... {rest} more byte{}", if rest > 1 { "s" } else { "" }));
    }
    let name = if is_buffer_value(c, this)? { "Buffer" } else { "Uint8Array" };
    let text = if text.is_empty() { format!("<{name} >") } else { format!("<{name} {text}>") };
    Ok(c.string_rooted(&text))
}

/// `buf.parent` and `buf.offset` (deprecated aliases of `buffer` and `byteOffset`); the data
/// names the property.
pub(crate) fn alias_get(c: &mut Context<'_>, this: RootId, _: &[RootId]) -> Result<RootId, RootedError> {
    if !is_buffer_value(c, this)? {
        return Ok(c.undefined());
    }
    let data = c.host_function_data()?;
    let name = c.string_text(data)?.unwrap_or_default();
    get(c, this, &name)
}

/// One numeric accessor of `Buffer.prototype`.
#[derive(Clone, Copy)]
enum Number {
    Unsigned,
    Signed,
    Float,
    BigUnsigned,
    BigSigned,
}

#[derive(Clone, Copy)]
struct Accessor {
    name: &'static str,
    write: bool,
    kind: Number,
    /// Bytes per value; zero for `readUIntLE`-style accessors that take a byte count.
    width: usize,
    little: bool,
}

const fn accessor(name: &'static str, write: bool, kind: Number, width: usize, little: bool) -> Accessor {
    Accessor { name, write, kind, width, little }
}

/// `Buffer.prototype`'s numeric readers and writers.
const ACCESSORS: &[Accessor] = &[
    accessor("readUInt8", false, Number::Unsigned, 1, true),
    accessor("readUInt16LE", false, Number::Unsigned, 2, true),
    accessor("readUInt16BE", false, Number::Unsigned, 2, false),
    accessor("readUInt32LE", false, Number::Unsigned, 4, true),
    accessor("readUInt32BE", false, Number::Unsigned, 4, false),
    accessor("readUIntLE", false, Number::Unsigned, 0, true),
    accessor("readUIntBE", false, Number::Unsigned, 0, false),
    accessor("readInt8", false, Number::Signed, 1, true),
    accessor("readInt16LE", false, Number::Signed, 2, true),
    accessor("readInt16BE", false, Number::Signed, 2, false),
    accessor("readInt32LE", false, Number::Signed, 4, true),
    accessor("readInt32BE", false, Number::Signed, 4, false),
    accessor("readIntLE", false, Number::Signed, 0, true),
    accessor("readIntBE", false, Number::Signed, 0, false),
    accessor("readFloatLE", false, Number::Float, 4, true),
    accessor("readFloatBE", false, Number::Float, 4, false),
    accessor("readDoubleLE", false, Number::Float, 8, true),
    accessor("readDoubleBE", false, Number::Float, 8, false),
    accessor("readBigUInt64LE", false, Number::BigUnsigned, 8, true),
    accessor("readBigUInt64BE", false, Number::BigUnsigned, 8, false),
    accessor("readBigInt64LE", false, Number::BigSigned, 8, true),
    accessor("readBigInt64BE", false, Number::BigSigned, 8, false),
    accessor("writeUInt8", true, Number::Unsigned, 1, true),
    accessor("writeUInt16LE", true, Number::Unsigned, 2, true),
    accessor("writeUInt16BE", true, Number::Unsigned, 2, false),
    accessor("writeUInt32LE", true, Number::Unsigned, 4, true),
    accessor("writeUInt32BE", true, Number::Unsigned, 4, false),
    accessor("writeUIntLE", true, Number::Unsigned, 0, true),
    accessor("writeUIntBE", true, Number::Unsigned, 0, false),
    accessor("writeInt8", true, Number::Signed, 1, true),
    accessor("writeInt16LE", true, Number::Signed, 2, true),
    accessor("writeInt16BE", true, Number::Signed, 2, false),
    accessor("writeInt32LE", true, Number::Signed, 4, true),
    accessor("writeInt32BE", true, Number::Signed, 4, false),
    accessor("writeIntLE", true, Number::Signed, 0, true),
    accessor("writeIntBE", true, Number::Signed, 0, false),
    accessor("writeFloatLE", true, Number::Float, 4, true),
    accessor("writeFloatBE", true, Number::Float, 4, false),
    accessor("writeDoubleLE", true, Number::Float, 8, true),
    accessor("writeDoubleBE", true, Number::Float, 8, false),
    accessor("writeBigUInt64LE", true, Number::BigUnsigned, 8, true),
    accessor("writeBigUInt64BE", true, Number::BigUnsigned, 8, false),
    accessor("writeBigInt64LE", true, Number::BigSigned, 8, true),
    accessor("writeBigInt64BE", true, Number::BigSigned, 8, false),
];

/// `boundsError(value, length[, type])`.
fn bounds_error(c: &mut Context<'_>, value: RootId, last: isize, name: Option<&str>) -> Result<RootedError, RootedError> {
    let number = validate_number(c, value, name.unwrap_or("offset"))?;
    let label = name.unwrap_or("offset");
    if number.floor() != number {
        return out_of_range(c, label, "an integer", value);
    }
    if last < 0 {
        return out_of_bounds(c, None);
    }
    let minimum = if name.is_some() { 1 } else { 0 };
    out_of_range(c, label, &format!(">= {minimum} and <= {last}"), value)
}

/// The accessor's offset, checked as Node checks it: a number naming `width` bytes inside the
/// buffer. Only the fixed-width accessors default it to 0.
fn checked_offset(c: &mut Context<'_>, offset: RootId, width: usize, length: usize, defaulted: bool) -> Result<usize, RootedError> {
    let number = if defaulted && is_undefined(c, offset) { 0.0 } else { validate_number(c, offset, "offset")? };
    if number.floor() != number || number < 0.0 || number + width as f64 > length as f64 {
        return Err(bounds_error(c, offset, length as isize - width as isize, None)?);
    }
    Ok(number as usize)
}

/// The byte count a `readUIntLE`-style accessor takes.
fn checked_width(c: &mut Context<'_>, width: RootId) -> Result<usize, RootedError> {
    if is_undefined(c, width) {
        return Err(super::util_shared_vm::argument_type_error(c, width, "byteLength", "number")?);
    }
    let number = validate_number(c, width, "byteLength")?;
    if number.floor() != number || !(1.0..=MAX_VARIABLE_WIDTH as f64).contains(&number) {
        return Err(bounds_error(c, width, MAX_VARIABLE_WIDTH as isize, Some("byteLength"))?);
    }
    Ok(number as usize)
}

/// The range text of `checkInt`'s `ERR_OUT_OF_RANGE`.
fn integer_range(width: usize, signed: bool, big: bool) -> String {
    let suffix = if big { "n" } else { "" };
    let bits = width * 8;
    if width > 4 {
        if signed {
            format!(">= -(2{suffix} ** {}{suffix}) and < 2 ** {}{suffix}", bits - 1, bits - 1)
        } else {
            format!(">= 0{suffix} and < 2{suffix} ** {bits}{suffix}")
        }
    } else {
        let (minimum, maximum) = integer_limits(width, signed);
        format!(">= {minimum}{suffix} and <= {maximum}{suffix}")
    }
}

fn integer_limits(width: usize, signed: bool) -> (i128, i128) {
    let bits = (width * 8) as u32;
    if signed {
        (-(1i128 << (bits - 1)), (1i128 << (bits - 1)) - 1)
    } else {
        (0, (1i128 << bits) - 1)
    }
}

fn read_bytes(bytes: &[u8], little: bool) -> u128 {
    let mut value = 0u128;
    if little {
        for (index, byte) in bytes.iter().enumerate() {
            value |= u128::from(*byte) << (8 * index);
        }
    } else {
        for byte in bytes {
            value = (value << 8) | u128::from(*byte);
        }
    }
    value
}

fn write_bytes(bytes: &mut [u8], value: u128, little: bool) {
    let count = bytes.len();
    for index in 0..count {
        let byte = (value >> (8 * index)) as u8;
        if little { bytes[index] = byte } else { bytes[count - 1 - index] = byte }
    }
}

/// Every numeric reader and writer of `Buffer.prototype`; the data is its `ACCESSORS` index.
pub(crate) fn numeric(c: &mut Context<'_>, this: RootId, args: &[RootId]) -> Result<RootId, RootedError> {
    let data = c.host_function_data()?;
    let spec = ACCESSORS[number_of(c, data).unwrap_or_default() as usize];
    let length = length_of(c, this);
    let (value, offset, width) = if spec.write {
        let width = if spec.width == 0 { let given = arg(c, args, 2); checked_width(c, given)? } else { spec.width };
        (Some(arg(c, args, 0)), arg(c, args, 1), width)
    } else {
        let width = if spec.width == 0 { let given = arg(c, args, 1); checked_width(c, given)? } else { spec.width };
        (None, arg(c, args, 0), width)
    };
    let Some(value) = value else {
        let offset = checked_offset(c, offset, width, length, spec.width != 0)?;
        let bytes = c.view_bytes_rooted(this).unwrap_or_default();
        let raw = read_bytes(&bytes[offset..offset + width], spec.little);
        let bits = (width * 8) as u32;
        return Ok(match spec.kind {
            Number::Unsigned => c.number(raw as f64),
            Number::Signed => {
                let shift = 128 - bits;
                c.number((((raw as i128) << shift) >> shift) as f64)
            }
            Number::Float if width == 4 => c.number(f64::from(f32::from_bits(raw as u32))),
            Number::Float => c.number(f64::from_bits(raw as u64)),
            Number::BigUnsigned => c.bigint_rooted(raw as i128),
            Number::BigSigned => c.bigint_rooted(i128::from(raw as u64 as i64)),
        });
    };
    let raw = match spec.kind {
        Number::Float => {
            let number = c.to_number(value)?;
            let offset = checked_offset(c, offset, width, length, spec.width != 0)?;
            let raw = if width == 4 { u128::from((number as f32).to_bits()) } else { u128::from(number.to_bits()) };
            return store(c, this, offset, width, raw, spec.little);
        }
        Number::Unsigned | Number::Signed => {
            let number = c.to_number(value)?;
            if spec.width == 1 && !is_undefined(c, offset) {
                validate_number(c, offset, "offset")?;
            }
            let (minimum, maximum) = integer_limits(width, matches!(spec.kind, Number::Signed));
            if number > maximum as f64 || number < minimum as f64 {
                let range = integer_range(width, matches!(spec.kind, Number::Signed), false);
                return Err(out_of_range(c, "value", &range, value)?);
            }
            let integer = if number.is_finite() { number.trunc() as i128 } else { 0 };
            integer as u128
        }
        Number::BigUnsigned | Number::BigSigned => {
            if c.value_type_rooted(value) != Some(quench_runtime::ValueType::BigInt) {
                let error = c.type_error_rooted("Cannot mix BigInt and other types, use explicit conversions")?;
                return Err(c.throw(error));
            }
            let integer: i128 = c.to_string(value)?.parse().unwrap_or_default();
            let signed = matches!(spec.kind, Number::BigSigned);
            let (minimum, maximum) = integer_limits(width, signed);
            if integer > maximum || integer < minimum {
                let range = integer_range(width, signed, true);
                return Err(out_of_range(c, "value", &range, value)?);
            }
            integer as u128
        }
    };
    let offset = checked_offset(c, offset, width, length, spec.width != 0)?;
    store(c, this, offset, width, raw, spec.little)
}

fn store(c: &mut Context<'_>, view: RootId, offset: usize, width: usize, raw: u128, little: bool) -> Result<RootId, RootedError> {
    c.with_buffer_bytes_mut_rooted(view, |bytes| write_bytes(&mut bytes[offset..offset + width], raw, little));
    Ok(c.number((offset + width) as f64))
}

/// `SlowBuffer(size)` (deprecated): an unpooled Buffer.
pub(crate) fn slow_buffer(c: &mut Context<'_>, this: RootId, args: &[RootId]) -> Result<RootId, RootedError> {
    alloc_unsafe(c, this, args)
}

/// Defines the `Buffer` class: a constructor inheriting from `Uint8Array`, its statics and
/// its prototype.
pub(crate) fn build_class(c: &mut Context<'_>) -> Result<RootId, RootedError> {
    let operation = crate::host::shared_vm::operation;
    let buffer = c.host_constructor_named(operation("bufferClassConstructor"), "Buffer")?;
    let prototype = get(c, buffer, "prototype")?;
    let global = c.global_root()?;
    let uint8 = get(c, global, "Uint8Array")?;
    let uint8_prototype = get(c, uint8, "prototype")?;
    c.set_prototype_rooted(buffer, uint8)?;
    c.set_prototype_rooted(prototype, uint8_prototype)?;
    let method = |c: &mut Context<'_>, target: RootId, name: &str, operation_name: &str, data: Option<RootId>| -> Result<RootId, RootedError> {
        let function = match data {
            Some(data) => c.host_function_named_with_data(operation(operation_name), name, data)?,
            None => c.host_function_named(operation(operation_name), name)?,
        };
        // Node assigns its Buffer methods, so they are ordinary enumerable data properties.
        let key = c.string_rooted(name);
        c.define_data_property_rooted(target, key, function, true, true, true)?;
        Ok(function)
    };
    for (name, operation_name) in [
        ("from", "bufferFrom"),
        ("of", "bufferOf"),
        ("alloc", "bufferAlloc"),
        ("allocUnsafe", "bufferAllocUnsafe"),
        ("allocUnsafeSlow", "bufferAllocUnsafe"),
        ("isBuffer", "bufferIsBuffer"),
        ("isEncoding", "bufferIsEncoding"),
        ("byteLength", "bufferByteLength"),
        ("compare", "bufferStaticCompare"),
        ("concat", "bufferConcat"),
        ("copyBytesFrom", "bufferCopyBytesFrom"),
    ] {
        method(c, buffer, name, operation_name, None)?;
    }
    let pool = c.number(POOL_SIZE);
    c.assign_named_property_rooted(buffer, "poolSize", pool)?;
    for (name, operation_name) in [
        ("toString", "bufferToString"),
        ("toLocaleString", "bufferToString"),
        ("equals", "bufferEquals"),
        ("compare", "bufferCompare"),
        ("copy", "bufferCopy"),
        ("fill", "bufferFill"),
        ("write", "bufferWrite"),
        ("toJSON", "bufferToJson"),
        ("subarray", "bufferSlice"),
        ("slice", "bufferSlice"),
        ("inspect", "bufferInspect"),
    ] {
        method(c, prototype, name, operation_name, None)?;
    }
    for (name, mode) in [("indexOf", SEARCH_FIRST), ("lastIndexOf", SEARCH_LAST), ("includes", SEARCH_INCLUDES)] {
        let data = c.number(f64::from(mode));
        method(c, prototype, name, "bufferIndexOf", Some(data))?;
    }
    for (name, width) in [("swap16", 2.0), ("swap32", 4.0), ("swap64", 8.0)] {
        let data = c.number(width);
        method(c, prototype, name, "bufferSwap", Some(data))?;
    }
    for encoding in ["ascii", "base64", "base64url", "latin1", "hex", "ucs2", "utf8"] {
        let data = c.string_rooted(encoding);
        method(c, prototype, &format!("{encoding}Slice"), "bufferEncodingSlice", Some(data))?;
        method(c, prototype, &format!("{encoding}Write"), "bufferEncodingWrite", Some(data))?;
    }
    for (index, accessor) in ACCESSORS.iter().enumerate() {
        let data = c.number(index as f64);
        let function = method(c, prototype, accessor.name, "bufferNumeric", Some(data))?;
        // Node aliases each `UInt` accessor as `Uint`: the same function under a second name.
        if accessor.name.contains("UInt") {
            let alias = c.string_rooted(&accessor.name.replace("UInt", "Uint"));
            c.define_data_property_rooted(prototype, alias, function, true, true, true)?;
        }
    }
    let util = get(c, global, "Symbol")?;
    let registry = get(c, util, "for")?;
    let custom = c.string_rooted("nodejs.util.inspect.custom");
    let custom = c.call_rooted(registry, util, &[custom])?;
    let inspect = c.host_function_named(operation("bufferInspect"), "inspect")?;
    c.define_data_property_rooted(prototype, custom, inspect, true, true, true)?;
    // `Buffer[Symbol.species]` is `FastBuffer`, which typed-array methods construct without
    // `Buffer()`'s deprecation; the two share one prototype.
    let fast = c.host_constructor_named(operation("bufferFastConstructor"), "FastBuffer")?;
    c.set_prototype_rooted(fast, uint8)?;
    c.define_named_data_property_rooted(fast, "prototype", prototype, false, false, false)?;
    let species_getter = c.host_function_named_with_data(operation("streamConstant"), "get", fast)?;
    let species = get(c, util, "species")?;
    let descriptor = c.null_object_rooted()?;
    c.assign_named_property_rooted(descriptor, "get", species_getter)?;
    let yes = c.boolean(true);
    c.assign_named_property_rooted(descriptor, "configurable", yes)?;
    c.define_property_descriptor_rooted(buffer, species, descriptor)?;
    for (name, target) in [("parent", "buffer"), ("offset", "byteOffset")] {
        let data = c.string_rooted(target);
        let getter = c.host_function_named_with_data(operation("bufferAliasGet"), "get", data)?;
        let descriptor = c.null_object_rooted()?;
        c.assign_named_property_rooted(descriptor, "get", getter)?;
        let yes = c.boolean(true);
        c.assign_named_property_rooted(descriptor, "enumerable", yes)?;
        c.assign_named_property_rooted(descriptor, "configurable", yes)?;
        let key = c.string_rooted(name);
        c.define_property_descriptor_rooted(prototype, key, descriptor)?;
    }
    Ok(buffer)
}

/// The `buffer` module's exports.
pub(crate) fn exports(c: &mut Context<'_>, buffer: RootId) -> Result<RootId, RootedError> {
    let operation = crate::host::shared_vm::operation;
    let module = c.object_rooted()?;
    c.assign_named_property_rooted(module, "Buffer", buffer)?;
    let slow = c.host_function_named(operation("bufferSlowBuffer"), "SlowBuffer")?;
    c.assign_named_property_rooted(module, "SlowBuffer", slow)?;
    let max_length = c.number(MAX_LENGTH);
    let max_string = c.number(Context::MAX_STRING_CODE_UNITS as f64);
    c.assign_named_property_rooted(module, "kMaxLength", max_length)?;
    c.assign_named_property_rooted(module, "kStringMaxLength", max_string)?;
    let constants = c.object_rooted()?;
    c.assign_named_property_rooted(constants, "MAX_LENGTH", max_length)?;
    c.assign_named_property_rooted(constants, "MAX_STRING_LENGTH", max_string)?;
    c.assign_named_property_rooted(module, "constants", constants)?;
    let inspect_max = c.number(INSPECT_MAX_BYTES as f64);
    c.assign_named_property_rooted(module, "INSPECT_MAX_BYTES", inspect_max)?;
    let binding = binding::binding(c)?;
    for (name, source) in [("atob", "atob"), ("btoa", "btoa"), ("isUtf8", "isUtf8"), ("isAscii", "isAscii")] {
        let value = get(c, binding, source)?;
        c.assign_named_property_rooted(module, name, value)?;
    }
    let (blob, file) = crate::modules::blob_shared_vm::classes(c)?;
    let resolve = crate::modules::blob_shared_vm::resolve_object_url_function(c)?;
    for (name, value) in [("Blob", blob), ("resolveObjectURL", resolve), ("File", file)] {
        c.assign_named_property_rooted(module, name, value)?;
    }
    Ok(module)
}
