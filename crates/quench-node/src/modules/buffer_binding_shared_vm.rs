//! `internalBinding('buffer')` (src/node_buffer.cc): the byte, string and search
//! primitives Node's lib/buffer.js and lib/internal/buffer.js build `Buffer` from.
use super::buffer_codec as codec;
use super::string_decoder_codec::ENCODINGS;
use crate::host::NodeHost;
use quench_runtime::{BuiltinBrand, NativeContext, RootId, RootedError};
type Context<'a> = NativeContext<'a, NodeHost>;

/// `kMaxLength`: v8::TypedArray::kMaxByteLength on 64-bit hosts, the largest integer
/// a double represents exactly.
const MAX_LENGTH: u64 = (1 << f64::MANTISSA_DIGITS) - 1;
/// Bytes per UCS-2 code unit.
const UCS2_WIDTH: usize = std::mem::size_of::<u16>();
/// The largest code unit Node's latin1 encoding represents.
const LATIN1_MAX: u16 = 0xff;
/// Bits per byte, for unpacking the little-endian WebSocket mask word.
const MASK_BYTE_BITS: u32 = u8::BITS;
/// Bytes in a WebSocket mask (RFC 6455, section 5.3).
const MASK_BYTES: usize = std::mem::size_of::<u32>();
/// Node's `fill` results lib/buffer.js turns into errors.
const FILL_INVALID_VALUE: f64 = -1.0;
const FILL_OUT_OF_BOUNDS: f64 = -2.0;
/// `atob` failure results (lib/buffer.js `atob`).
const ATOB_REMAINDER: f64 = -1.0;
const ATOB_INVALID_CHARACTER: f64 = -2.0;
/// `btoa`'s result for input outside latin1.
const BTOA_INVALID_CHARACTER: f64 = -1.0;
/// Each complete forgiving-base64 quantum is four characters.
const BASE64_QUANTUM: usize = 4;
const BASE64_PADDING: u8 = b'=';

/// How node_buffer.cc's `Initialize` registers a native, which decides its `name`.
#[derive(Clone, Copy)]
enum Registration {
    /// `SetMethod`: named after its binding key.
    Method,
    /// `SetFastMethod`: a V8 fast-API function, which is anonymous.
    Fast,
}

/// The natives, by binding key, private operation and registration.
const NATIVES: &[(&str, &str, Registration)] = &[
    ("atob", "bufferBindingAtob", Registration::Method),
    ("btoa", "bufferBindingBtoa", Registration::Method),
    ("byteLengthUtf8", "bufferBindingByteLengthUtf8", Registration::Fast),
    ("copy", "bufferBindingCopy", Registration::Fast),
    ("compare", "bufferBindingCompare", Registration::Fast),
    ("compareOffset", "bufferBindingCompareOffset", Registration::Method),
    ("fill", "bufferBindingFill", Registration::Method),
    ("indexOfBuffer", "bufferBindingIndexOfBuffer", Registration::Method),
    ("indexOfNumber", "bufferBindingIndexOfNumber", Registration::Fast),
    ("indexOfString", "bufferBindingIndexOfString", Registration::Method),
    ("copyArrayBuffer", "bufferBindingCopyArrayBuffer", Registration::Fast),
    ("createUnsafeArrayBuffer", "bufferBindingCreateUnsafeArrayBuffer", Registration::Method),
    ("arrayBufferAlignedOffset", "bufferBindingArrayBufferAlignedOffset", Registration::Method),
    ("swap16", "bufferBindingSwap16", Registration::Fast),
    ("swap32", "bufferBindingSwap32", Registration::Fast),
    ("swap64", "bufferBindingSwap64", Registration::Fast),
    ("isUtf8", "bufferBindingIsUtf8", Registration::Fast),
    ("isAscii", "bufferBindingIsAscii", Registration::Fast),
    ("isLatin1", "bufferBindingIsLatin1", Registration::Fast),
    ("mask", "bufferBindingMask", Registration::Fast),
    ("stringLengthUtf8", "bufferBindingStringLengthUtf8", Registration::Fast),
    ("asciiSlice", "bufferBindingAsciiSlice", Registration::Method),
    ("base64Slice", "bufferBindingBase64Slice", Registration::Method),
    ("base64urlSlice", "bufferBindingBase64urlSlice", Registration::Method),
    ("latin1Slice", "bufferBindingLatin1Slice", Registration::Method),
    ("hexSlice", "bufferBindingHexSlice", Registration::Method),
    ("ucs2Slice", "bufferBindingUcs2Slice", Registration::Method),
    ("utf8Slice", "bufferBindingUtf8Slice", Registration::Method),
    ("base64Write", "bufferBindingBase64Write", Registration::Method),
    ("base64urlWrite", "bufferBindingBase64urlWrite", Registration::Method),
    ("hexWrite", "bufferBindingHexWrite", Registration::Method),
    ("ucs2Write", "bufferBindingUcs2Write", Registration::Method),
    ("asciiWriteStatic", "bufferBindingAsciiWrite", Registration::Fast),
    ("latin1WriteStatic", "bufferBindingLatin1Write", Registration::Fast),
    ("utf8WriteStatic", "bufferBindingUtf8Write", Registration::Fast),
    ("setDetachKey", "bufferBindingSetDetachKey", Registration::Method),
];

pub(crate) fn binding(c: &mut Context<'_>) -> Result<RootId, RootedError> {
    crate::host::shared_vm::commonjs::cached_native_binding(c, "\0binding:buffer", |c| {
        c.with_startup_intrinsics(|c| {
            let binding = c.object_rooted()?;
            let global = c.global_root()?;
            let array_buffer = c.get_named_property_rooted(global, "ArrayBuffer")?;
            for &(name, operation, registration) in NATIVES {
                let operation = crate::host::shared_vm::operation(operation);
                let display = match registration {
                    Registration::Method => name,
                    Registration::Fast => "",
                };
                let function = c.host_function_named_with_data(operation, display, array_buffer)?;
                put(c, binding, name, function)?;
            }
            let max_length = c.number(MAX_LENGTH as f64);
            put(c, binding, "kMaxLength", max_length)?;
            let max_string_length = c.number(Context::MAX_STRING_CODE_UNITS as f64);
            put(c, binding, "kStringMaxLength", max_string_length)?;
            Ok(binding)
        })
    })
}

fn put(c: &mut Context<'_>, object: RootId, name: &str, value: RootId) -> Result<(), RootedError> {
    let key = c.string_rooted(name);
    c.define_data_property_rooted(object, key, value, true, true, true)?;
    Ok(())
}

fn arg(c: &mut Context<'_>, args: &[RootId], index: usize) -> RootId {
    match args.get(index) {
        Some(value) => *value,
        None => c.undefined(),
    }
}

/// The error classes node_errors.h throws, each a plain error with an own `code`.
enum Class {
    Error,
    Type,
    Range,
}

fn throw(c: &mut Context<'_>, class: Class, code: &str, message: &str) -> RootedError {
    let created = match class {
        Class::Error => c.error_rooted(message),
        Class::Type => c.type_error_rooted(message),
        Class::Range => c.range_error_rooted(message),
    };
    let error = match created {
        Ok(error) => error,
        Err(error) => return error,
    };
    let key = c.string_rooted("code");
    let value = c.string_rooted(code);
    if let Err(error) = c.set_property_rooted(error, key, value, error) {
        return error;
    }
    c.throw(error)
}

/// `THROW_AND_RETURN_IF_OOB`.
fn index_out_of_range(c: &mut Context<'_>) -> RootedError {
    throw(c, Class::Range, "ERR_OUT_OF_RANGE", "Index out of range")
}

/// `ERR_STRING_TOO_LONG` for a decoding past the engine's string length.
fn string_too_long(c: &mut Context<'_>) -> RootedError {
    let message = format!("Cannot create a string longer than 0x{:x} characters", Context::MAX_STRING_CODE_UNITS);
    throw(c, Class::Error, "ERR_STRING_TOO_LONG", &message)
}

/// `THROW_AND_RETURN_UNLESS_BUFFER`: the byte length of an ArrayBufferView.
fn view_length(c: &mut Context<'_>, value: RootId) -> Result<usize, RootedError> {
    if !c.has_builtin_brand_rooted(value, BuiltinBrand::ArrayBufferView)? {
        return Err(throw(c, Class::Type, "ERR_INVALID_ARG_TYPE", "argument must be a buffer"));
    }
    Ok(byte_length(c, value))
}

/// The current byte length of an ArrayBuffer or view; zero once detached.
fn byte_length(c: &Context<'_>, value: RootId) -> usize {
    c.with_buffer_bytes_rooted(value, <[u8]>::len).unwrap_or_default()
}

/// `THROW_AND_RETURN_IF_NOT_STRING(env, value, "argument")`.
fn string_units(c: &mut Context<'_>, value: RootId) -> Result<Vec<u16>, RootedError> {
    match c.string_code_units(value)? {
        Some(units) => Ok(units),
        None => Err(not_a_string(c)),
    }
}

/// The TypeError a binding that takes a string throws for any other argument.
fn not_a_string(c: &mut Context<'_>) -> RootedError {
    throw(c, Class::Type, "ERR_INVALID_ARG_TYPE", "argument must be a string")
}

/// `ParseArrayIndex`: `default` for undefined, `None` for a negative integer.
fn parse_array_index(c: &mut Context<'_>, value: RootId, default: usize) -> Result<Option<usize>, RootedError> {
    if c.rooted_value(value).is_some_and(|value| value.is_undefined()) {
        return Ok(Some(default));
    }
    let integer = integer(c, value)?;
    Ok((integer >= 0.0).then(|| integer.min(usize::MAX as f64) as usize))
}

fn index(c: &mut Context<'_>, value: RootId, default: usize) -> Result<usize, RootedError> {
    parse_array_index(c, value, default)?.ok_or_else(|| index_out_of_range(c))
}

/// `Value::IntegerValue`: ToNumber truncated toward zero, NaN as zero.
fn integer(c: &mut Context<'_>, value: RootId) -> Result<f64, RootedError> {
    let number = c.to_number(value)?;
    Ok(if number.is_nan() { 0.0 } else { number.trunc() })
}

/// An argument lib/buffer.js has already validated as a non-negative size.
fn size(c: &mut Context<'_>, args: &[RootId], position: usize) -> Result<usize, RootedError> {
    let value = arg(c, args, position);
    Ok(integer(c, value)?.clamp(0.0, usize::MAX as f64) as usize)
}

/// `args[i].As<Integer>()->Value()` for a validated numeric argument.
fn signed(c: &mut Context<'_>, args: &[RootId], position: usize) -> Result<i64, RootedError> {
    let value = arg(c, args, position);
    Ok(integer(c, value)? as i64)
}

/// The encoding behind the `encodingsMap` id lib/buffer.js passes.
fn encoding_id(c: &mut Context<'_>, args: &[RootId], position: usize) -> Result<&'static str, RootedError> {
    let id = size(c, args, position)?;
    Ok(match ENCODINGS.get(id) {
        Some(&"buffer") | None => "utf8",
        Some(name) => name,
    })
}

/// `StringBytes::Encode`: decoded bytes as a string, within the engine's length limit.
pub(crate) fn decode_bytes(c: &mut Context<'_>, bytes: &[u8], encoding: &str) -> Result<RootId, RootedError> {
    decoded_string(c, bytes, encoding).ok_or_else(|| string_too_long(c))
}

/// `bytes` decoded under `encoding` as a string, or `None` past the longest string.
fn decoded_string(c: &mut Context<'_>, bytes: &[u8], encoding: &str) -> Option<RootId> {
    match decode_text(bytes, encoding)? {
        DecodedText::Direct(text) => Some(c.decoded_string_rooted(text)),
        DecodedText::Units(units) => Some(c.string_units_rooted(&units)),
    }
}

/// A decoding's result: text the bytes spell directly, or UTF-16 code units.
enum DecodedText<'a> {
    Direct(&'a str),
    Units(Vec<u16>),
}

/// `bytes` decoded under `encoding`, or `None` past the longest string. Bytes that spell
/// their text directly (valid UTF-8 under `utf8`, ASCII under `latin1` and `ascii`) are the
/// text itself; the rest decode through UTF-16 code units (`codec::decode_units_limited`).
/// A 176 KB ASCII JSON file read as UTF-8 took 0.78 ms through the code units, against
/// Bun's 0.12-0.16 (x86_64 VM, 2026-10-23).
fn decode_text<'a>(bytes: &'a [u8], encoding: &str) -> Option<DecodedText<'a>> {
    let maximum = Context::MAX_STRING_CODE_UNITS;
    let direct = match encoding {
        "utf8" => std::str::from_utf8(bytes).ok(),
        "latin1" | "ascii" if bytes.is_ascii() => std::str::from_utf8(bytes).ok(),
        _ => None,
    };
    match direct {
        // A UTF-8 text has at most one UTF-16 code unit per byte.
        Some(text) if text.len() <= maximum => Some(DecodedText::Direct(text)),
        _ => codec::decode_units_limited(bytes, encoding, maximum).map(DecodedText::Units),
    }
}

/// `StringSlice<encoding>(buffer, start, end)`.
fn slice(c: &mut Context<'_>, args: &[RootId], encoding: &str) -> Result<RootId, RootedError> {
    let view = arg(c, args, 0);
    let length = view_length(c, view)?;
    if length == 0 {
        return Ok(c.string_rooted(""));
    }
    let start = arg(c, args, 1);
    let start = index(c, start, 0)?;
    let end = arg(c, args, 2);
    let end = index(c, end, length)?;
    if end <= start {
        return Ok(c.string_rooted(""));
    }
    if end > length {
        return Err(index_out_of_range(c));
    }
    // The text leaves the buffer's borrow as an owned copy before the string is made.
    let decoded = c.with_buffer_bytes_rooted(view, |bytes| {
        decode_text(bytes.get(start..end).unwrap_or_default(), encoding).map(|decoded| match decoded {
            DecodedText::Direct(text) => Ok(text.to_owned()),
            DecodedText::Units(units) => Err(units),
        })
    });
    match decoded.flatten() {
        Some(Ok(text)) => Ok(c.decoded_string_rooted(&text)),
        Some(Err(units)) => Ok(c.string_units_rooted(&units)),
        None => Err(string_too_long(c)),
    }
}

/// The longest prefix of `encoded` that fits `capacity` without splitting a character
/// (`StringBytes::Write`).
fn writable_prefix(encoded: &[u8], encoding: &str, capacity: usize) -> usize {
    let mut count = capacity.min(encoded.len());
    match encoding {
        "utf8" => {
            // A UTF-8 continuation byte never starts a character.
            while count < encoded.len() && encoded[count] & 0xc0 == 0x80 {
                count -= 1;
            }
        }
        "utf16le" => count -= count % UCS2_WIDTH,
        _ => {}
    }
    count
}

/// The bytes `string` encodes to under `encoding`; UTF-8 straight from the string's storage
/// (`NativeContext::string_utf8`), which an ASCII string holds as is. A non-string throws.
fn encoded_string(c: &mut Context<'_>, string: RootId, encoding: &str) -> Result<Vec<u8>, RootedError> {
    if encoding == "utf8" {
        return c.string_utf8(string)?.ok_or_else(|| not_a_string(c));
    }
    let units = string_units(c, string)?;
    Ok(codec::encode_units(&units, encoding))
}

/// `StringWrite<encoding>(buffer, string, offset, length)`, also the static one-byte writes.
fn write(c: &mut Context<'_>, args: &[RootId], encoding: &str) -> Result<RootId, RootedError> {
    let view = arg(c, args, 0);
    let length = view_length(c, view)?;
    let string = arg(c, args, 1);
    let encoded = encoded_string(c, string, encoding)?;
    let offset = arg(c, args, 2);
    let offset = index(c, offset, 0)?;
    if offset > length {
        return Err(throw(c, Class::Range, "ERR_BUFFER_OUT_OF_BOUNDS", "\"offset\" is outside of buffer bounds"));
    }
    let remaining = length - offset;
    let capacity = arg(c, args, 3);
    let capacity = index(c, capacity, remaining)?.min(remaining);
    if capacity == 0 {
        return Ok(c.number(0.0));
    }
    let count = writable_prefix(&encoded, encoding, capacity);
    let written = c.with_buffer_bytes_mut_rooted(view, |bytes| {
        bytes[offset..offset + count].copy_from_slice(&encoded[..count]);
        count
    });
    Ok(c.number(written.unwrap_or_default() as f64))
}

macro_rules! codecs {
    ($($slice:ident, $write:ident => $encoding:literal;)*) => {$(
        pub(crate) fn $slice(c: &mut Context<'_>, _: RootId, args: &[RootId]) -> Result<RootId, RootedError> {
            slice(c, args, $encoding)
        }
        pub(crate) fn $write(c: &mut Context<'_>, _: RootId, args: &[RootId]) -> Result<RootId, RootedError> {
            write(c, args, $encoding)
        }
    )*};
}

codecs! {
    ascii_slice, ascii_write => "ascii";
    base64_slice, base64_write => "base64";
    base64url_slice, base64url_write => "base64url";
    latin1_slice, latin1_write => "latin1";
    hex_slice, hex_write => "hex";
    ucs2_slice, ucs2_write => "utf16le";
    utf8_slice, utf8_write => "utf8";
}

/// `byteLengthUtf8(string)`: lone surrogates count as U+FFFD.
pub(crate) fn byte_length_utf8(c: &mut Context<'_>, _: RootId, args: &[RootId]) -> Result<RootId, RootedError> {
    let string = arg(c, args, 0);
    let Some(length) = c.string_utf8_length(string)? else {
        return Err(not_a_string(c));
    };
    Ok(c.number(length as f64))
}

/// `copy(source, target, targetStart, sourceStart, count)`: the bytes copied, none into a
/// detached or immutable target.
pub(crate) fn copy(c: &mut Context<'_>, _: RootId, args: &[RootId]) -> Result<RootId, RootedError> {
    let source = arg(c, args, 0);
    let target = arg(c, args, 1);
    let target_start = size(c, args, 2)?;
    let source_start = size(c, args, 3)?;
    let count = size(c, args, 4)?;
    let copied = copy_bytes(c, source, source_start, target, target_start, count);
    Ok(c.number(copied as f64))
}

/// Copy between two buffers or views, clamped to both ranges. The source is read first,
/// so one buffer's lock is never held while the other is taken.
fn copy_bytes(c: &mut Context<'_>, source: RootId, source_start: usize, target: RootId, target_start: usize, count: usize) -> usize {
    let bytes = c.with_buffer_bytes_rooted(source, |bytes| {
        let start = source_start.min(bytes.len());
        bytes[start..start + count.min(bytes.len() - start)].to_vec()
    });
    let Some(bytes) = bytes else {
        return 0;
    };
    let written = c.with_buffer_bytes_mut_rooted(target, |target| {
        let start = target_start.min(target.len());
        let count = bytes.len().min(target.len() - start);
        target[start..start + count].copy_from_slice(&bytes[..count]);
        count
    });
    written.unwrap_or_default()
}

/// The bytes of a view for comparing against another buffer's.
fn view_bytes(c: &mut Context<'_>, value: RootId) -> Vec<u8> {
    c.view_bytes_rooted(value).unwrap_or_default()
}

/// `normalizeCompareVal`: memcmp's sign, or the length order when the prefix is equal.
fn compare_bytes(source: &[u8], target: &[u8], source_length: usize, target_length: usize) -> f64 {
    let ordering = source.cmp(target).then(source_length.cmp(&target_length));
    match ordering {
        std::cmp::Ordering::Less => -1.0,
        std::cmp::Ordering::Equal => 0.0,
        std::cmp::Ordering::Greater => 1.0,
    }
}

pub(crate) fn compare(c: &mut Context<'_>, _: RootId, args: &[RootId]) -> Result<RootId, RootedError> {
    let source = arg(c, args, 0);
    let target = arg(c, args, 1);
    view_length(c, source)?;
    view_length(c, target)?;
    let target = view_bytes(c, target);
    let order = c.with_buffer_bytes_rooted(source, |source| {
        let shared = source.len().min(target.len());
        compare_bytes(&source[..shared], &target[..shared], source.len(), target.len())
    });
    Ok(c.number(order.unwrap_or_default()))
}

/// `compareOffset(source, target, targetStart, sourceStart, targetEnd, sourceEnd)`.
pub(crate) fn compare_offset(c: &mut Context<'_>, _: RootId, args: &[RootId]) -> Result<RootId, RootedError> {
    let source = arg(c, args, 0);
    let target = arg(c, args, 1);
    let source_length = view_length(c, source)?;
    let target_length = view_length(c, target)?;
    let mut bounds = [0; 4];
    for (bound, (position, default)) in bounds.iter_mut().zip([(2, 0), (3, 0), (4, target_length), (5, source_length)]) {
        let value = arg(c, args, position);
        *bound = index(c, value, default)?;
    }
    let [target_start, source_start, target_end, source_end] = bounds;
    if source_start > source_length {
        return Err(throw(c, Class::Range, "ERR_OUT_OF_RANGE", "The value of \"sourceStart\" is out of range."));
    }
    if target_start > target_length {
        return Err(throw(c, Class::Range, "ERR_OUT_OF_RANGE", "The value of \"targetStart\" is out of range."));
    }
    let source_span = source_end.saturating_sub(source_start);
    let target_span = target_end.saturating_sub(target_start);
    let count = source_span.min(target_span).min(source_length - source_start);
    let target = view_bytes(c, target);
    let compared = target.get(target_start..target_start + count).map(<[u8]>::to_vec).unwrap_or_default();
    let order = c.with_buffer_bytes_rooted(source, |source| {
        let source = source.get(source_start..source_start + compared.len()).unwrap_or_default();
        compare_bytes(source, &compared, source_span, target_span)
    });
    Ok(c.number(order.unwrap_or_default()))
}

/// `fill(buffer, value, start, end, encoding)`: undefined, or Node's -1/-2 failure results.
pub(crate) fn fill(c: &mut Context<'_>, _: RootId, args: &[RootId]) -> Result<RootId, RootedError> {
    let view = arg(c, args, 0);
    let length = view_length(c, view)?;
    let start = arg(c, args, 2);
    let start = index(c, start, 0)?;
    let end = arg(c, args, 3);
    let end = index(c, end, 0)?;
    if start > end || end > length {
        return Ok(c.number(FILL_OUT_OF_BOUNDS));
    }
    let count = end - start;
    let value = arg(c, args, 1);
    let pattern = if c.has_builtin_brand_rooted(value, BuiltinBrand::ArrayBufferView)? {
        view_bytes(c, value)
    } else if let Some(units) = c.string_code_units(value)? {
        let encoding = arg(c, args, 4);
        let encoding = c.string_text(encoding)?;
        let encoding = encoding.as_deref().and_then(codec::canonical_encoding).unwrap_or("utf8");
        let mut encoded = codec::encode_units(&units, encoding);
        // Encodings other than UTF-8 and UCS-2 write whole characters into the range.
        if !matches!(encoding, "utf8" | "utf16le") {
            encoded.truncate(writable_prefix(&encoded, encoding, count));
        }
        encoded
    } else {
        let number = c.to_number(value)?;
        // `Uint32Value(value) & 255`: the low byte of ToUint32.
        let byte = if number.is_finite() { number.trunc().rem_euclid(f64::from(u32::MAX) + 1.0) as u32 as u8 } else { 0 };
        vec![byte]
    };
    if pattern.is_empty() && count > 0 {
        return Ok(c.number(FILL_INVALID_VALUE));
    }
    c.with_buffer_bytes_mut_rooted(view, |bytes| {
        for (byte, value) in bytes[start..end].iter_mut().zip(pattern.iter().cycle()) {
            *byte = *value;
        }
    });
    Ok(c.undefined())
}

/// `IndexOfOffset`: the search start inside the buffer, or `None` for no possible match.
fn index_of_offset(length: usize, offset: i64, needle: usize, forward: bool) -> Option<usize> {
    let length = length as i64;
    let needle = needle as i64;
    let start = if offset < 0 {
        if offset + length >= 0 {
            length + offset
        } else if forward || needle == 0 {
            0
        } else {
            return None;
        }
    } else if offset.saturating_add(needle) <= length {
        offset
    } else if needle == 0 {
        length
    } else if forward {
        return None;
    } else {
        length - 1
    };
    Some(start as usize)
}

/// `nbytes::SearchString` over `width`-byte units: the byte position of the first match
/// at or after `start`, or of the last at or before it.
fn search(haystack: &[u8], needle: &[u8], start: usize, forward: bool, width: usize) -> Option<usize> {
    let units = haystack.len() / width;
    let needle_units = needle.len() / width;
    let last = units.checked_sub(needle_units)?;
    let start = start / width;
    let needle = &needle[..needle_units * width];
    if width == 1 {
        return if forward {
            let from = haystack.get(start..)?;
            memchr::memmem::find(from, needle).map(|position| position + start)
        } else {
            memchr::memmem::rfind(&haystack[..start.min(last) + needle.len()], needle)
        };
    }
    let matches = |unit: &usize| haystack[unit * width..unit * width + needle.len()] == *needle;
    let found = if forward { (start..=last).find(matches) } else { (0..=start.min(last)).rev().find(matches) };
    found.map(|unit| unit * width)
}

/// How node_buffer.cc's two searches treat UCS-2 alignment, which they do differently.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Needle {
    /// `IndexOfString`: the haystack is cut to whole code units and a forward search
    /// starts at the next unit boundary.
    String,
    /// `IndexOfBuffer`: only the search end is cut to whole units.
    Buffer,
}

/// `IndexOfString` and `IndexOfBuffer` after the needle is encoded: Node's clamping of the
/// start and `end` around `search`.
fn index_of(haystack: &[u8], needle: &[u8], offset: i64, end: i64, forward: bool, width: usize, kind: Needle) -> f64 {
    let length = if kind == Needle::String { haystack.len() / width * width } else { haystack.len() };
    let search_end = (end.max(0) as usize).min(length) / width * width;
    let start = index_of_offset(length, offset, needle.len(), forward);
    if needle.is_empty() {
        return start.map_or(-1.0, |start| start.min(search_end) as f64);
    }
    let Some(mut start) = start.filter(|_| length > 0) else {
        return -1.0;
    };
    if start >= search_end {
        if forward || search_end == 0 {
            return -1.0;
        }
        start = search_end - 1;
    }
    if kind == Needle::String && width == UCS2_WIDTH && forward {
        start += start % UCS2_WIDTH;
        if start >= search_end {
            return -1.0;
        }
    }
    if (forward && needle.len() + start > search_end) || needle.len() > search_end || needle.len() < width {
        return -1.0;
    }
    search(&haystack[..search_end], needle, start, forward, width).map_or(-1.0, |position| position as f64)
}

/// The width of the units an `encodingsMap` encoding searches by.
fn search_width(encoding: &str) -> usize {
    if encoding == "utf16le" { UCS2_WIDTH } else { 1 }
}

/// `indexOfString(buffer, needle, offset, encoding, forward, end)`.
pub(crate) fn index_of_string(c: &mut Context<'_>, _: RootId, args: &[RootId]) -> Result<RootId, RootedError> {
    let view = arg(c, args, 0);
    let needle = arg(c, args, 1);
    let needle = string_units(c, needle)?;
    let offset = signed(c, args, 2)?;
    let encoding = encoding_id(c, args, 3)?;
    let forward = arg(c, args, 4);
    let forward = c.truthy_rooted(forward)?;
    let end = signed(c, args, 5)?;
    view_length(c, view)?;
    let needle = codec::encode_units(&needle, encoding);
    let width = search_width(encoding);
    let found = c.with_buffer_bytes_rooted(view, |haystack| index_of(haystack, &needle, offset, end, forward, width, Needle::String));
    Ok(c.number(found.unwrap_or(-1.0)))
}

/// `indexOfBuffer(buffer, needle, offset, encoding, forward, end)`.
pub(crate) fn index_of_buffer(c: &mut Context<'_>, _: RootId, args: &[RootId]) -> Result<RootId, RootedError> {
    let view = arg(c, args, 0);
    let needle = arg(c, args, 1);
    let offset = signed(c, args, 2)?;
    let encoding = encoding_id(c, args, 3)?;
    let forward = arg(c, args, 4);
    let forward = c.truthy_rooted(forward)?;
    let end = signed(c, args, 5)?;
    view_length(c, view)?;
    view_length(c, needle)?;
    let needle = view_bytes(c, needle);
    let width = search_width(encoding);
    let found = c.with_buffer_bytes_rooted(view, |haystack| index_of(haystack, &needle, offset, end, forward, width, Needle::Buffer));
    Ok(c.number(found.unwrap_or(-1.0)))
}

/// `indexOfNumber(buffer, byte, offset, forward, end)`.
pub(crate) fn index_of_number(c: &mut Context<'_>, _: RootId, args: &[RootId]) -> Result<RootId, RootedError> {
    let view = arg(c, args, 0);
    let needle = size(c, args, 1)? as u8;
    let offset = signed(c, args, 2)?;
    let forward = arg(c, args, 3);
    let forward = c.truthy_rooted(forward)?;
    let end = signed(c, args, 4)?;
    view_length(c, view)?;
    let found = c.with_buffer_bytes_rooted(view, |bytes| {
        let start = index_of_offset(bytes.len(), offset, 1, forward).filter(|_| !bytes.is_empty())?;
        let search_end = (end.max(0) as usize).min(bytes.len());
        if forward {
            let from = bytes.get(start..search_end)?;
            memchr::memchr(needle, from).map(|position| position + start)
        } else {
            memchr::memrchr(needle, &bytes[..(start + 1).min(search_end)])
        }
    });
    Ok(c.number(found.flatten().map_or(-1.0, |position| position as f64)))
}

/// `copyArrayBuffer(target, targetOffset, source, sourceOffset, count)`.
pub(crate) fn copy_array_buffer(c: &mut Context<'_>, _: RootId, args: &[RootId]) -> Result<RootId, RootedError> {
    let target = arg(c, args, 0);
    let target_start = size(c, args, 1)?;
    let source = arg(c, args, 2);
    let source_start = size(c, args, 3)?;
    let count = size(c, args, 4)?;
    if copy_bytes(c, source, source_start, target, target_start, count) != count {
        return Err(RootedError::host("copyArrayBuffer range exceeds its buffers"));
    }
    Ok(c.undefined())
}

/// `createUnsafeArrayBuffer(size)`; the VM's buffers are always zero-filled.
pub(crate) fn create_unsafe_array_buffer(c: &mut Context<'_>, _: RootId, args: &[RootId]) -> Result<RootId, RootedError> {
    if args.len() != 1 {
        let error = c.range_error_rooted("Invalid array buffer length")?;
        return Err(c.throw(error));
    }
    let constructor = c.host_function_data()?;
    c.construct_rooted(constructor, constructor, args)
}

/// `arrayBufferAlignedOffset(arrayBuffer, alignment)`; the alignment is validated in JS.
pub(crate) fn array_buffer_aligned_offset(c: &mut Context<'_>, _: RootId, args: &[RootId]) -> Result<RootId, RootedError> {
    let buffer = arg(c, args, 0);
    let alignment = size(c, args, 1)?;
    let offset = c.array_buffer_aligned_offset_rooted(buffer, alignment)?;
    Ok(c.number(offset as f64))
}

/// `setDetachKey(arrayBuffer, key)`: only a matching key may detach the buffer, which the VM
/// represents as its forbidden-transfer mark.
pub(crate) fn set_detach_key(c: &mut Context<'_>, _: RootId, args: &[RootId]) -> Result<RootId, RootedError> {
    let buffer = arg(c, args, 0);
    c.mark_untransferable_rooted(buffer)?;
    Ok(c.undefined())
}

fn swap(c: &mut Context<'_>, args: &[RootId], width: usize) -> Result<RootId, RootedError> {
    let view = arg(c, args, 0);
    c.with_buffer_bytes_mut_rooted(view, |bytes| bytes.chunks_exact_mut(width).for_each(<[u8]>::reverse));
    Ok(c.undefined())
}

macro_rules! swaps {
    ($($name:ident => $width:ty;)*) => {$(
        pub(crate) fn $name(c: &mut Context<'_>, _: RootId, args: &[RootId]) -> Result<RootId, RootedError> {
            swap(c, args, std::mem::size_of::<$width>())
        }
    )*};
}

swaps! {
    swap16 => u16;
    swap32 => u32;
    swap64 => u64;
}

fn validate(c: &mut Context<'_>, args: &[RootId], valid: fn(&[u8]) -> bool) -> Result<RootId, RootedError> {
    let value = arg(c, args, 0);
    let result = c.with_buffer_bytes_rooted(value, valid).unwrap_or(true);
    Ok(c.boolean(result))
}

pub(crate) fn is_utf8(c: &mut Context<'_>, _: RootId, args: &[RootId]) -> Result<RootId, RootedError> {
    validate(c, args, |bytes| std::str::from_utf8(bytes).is_ok())
}

pub(crate) fn is_ascii(c: &mut Context<'_>, _: RootId, args: &[RootId]) -> Result<RootId, RootedError> {
    validate(c, args, <[u8]>::is_ascii)
}

/// `isLatin1(string)`: every code unit fits Node's one-byte latin1.
pub(crate) fn is_latin1(c: &mut Context<'_>, _: RootId, args: &[RootId]) -> Result<RootId, RootedError> {
    let string = arg(c, args, 0);
    let units = string_units(c, string)?;
    let result = units.iter().all(|unit| *unit <= LATIN1_MAX);
    Ok(c.boolean(result))
}

/// `stringLengthUtf8(buffer)`: the UTF-16 length of the bytes decoded with maximal-subpart
/// U+FFFD replacement.
pub(crate) fn string_length_utf8(c: &mut Context<'_>, _: RootId, args: &[RootId]) -> Result<RootId, RootedError> {
    let value = arg(c, args, 0);
    let length = c.with_buffer_bytes_rooted(value, |bytes| String::from_utf8_lossy(bytes).chars().map(char::len_utf16).sum::<usize>());
    Ok(c.number(length.unwrap_or_default() as f64))
}

/// `mask(source, destination, offset, length, mask)`: XOR `length` source bytes into the
/// destination at `offset` with the little-endian mask word; false for an immutable
/// destination.
pub(crate) fn mask(c: &mut Context<'_>, _: RootId, args: &[RootId]) -> Result<RootId, RootedError> {
    let source = arg(c, args, 0);
    let destination = arg(c, args, 1);
    let offset = size(c, args, 2)?;
    let length = size(c, args, 3)?;
    let word = arg(c, args, 4);
    let word = c.to_number(word)? as u32;
    let pattern: [u8; MASK_BYTES] = std::array::from_fn(|index| (word >> (MASK_BYTE_BITS * index as u32)) as u8);
    let mut bytes = view_bytes(c, source);
    bytes.truncate(length);
    for (index, byte) in bytes.iter_mut().enumerate() {
        *byte ^= pattern[index % MASK_BYTES];
    }
    let written = c.with_buffer_bytes_mut_rooted(destination, |destination| {
        destination[offset..offset + bytes.len()].copy_from_slice(&bytes);
    });
    Ok(c.boolean(written.is_some()))
}

/// `btoa(string)`: base64 of a latin1 string, or -1 outside latin1.
pub(crate) fn btoa(c: &mut Context<'_>, _: RootId, args: &[RootId]) -> Result<RootId, RootedError> {
    let string = arg(c, args, 0);
    let units = string_units(c, string)?;
    let bytes: Option<Vec<u8>> = units.iter().map(|unit| u8::try_from(*unit).ok()).collect();
    Ok(match bytes {
        Some(bytes) => c.string_rooted(&codec::base64_encode(&bytes, true, false)),
        None => c.number(BTOA_INVALID_CHARACTER),
    })
}

/// WHATWG forgiving-base64 decode: the bytes, or Node's `atob` failure result.
fn forgiving_base64(units: &[u16]) -> Result<Vec<u8>, f64> {
    const ASCII_WHITESPACE: [u16; 5] = [0x09, 0x0a, 0x0c, 0x0d, 0x20];
    let mut data: Vec<u8> = Vec::with_capacity(units.len());
    for unit in units.iter().filter(|unit| !ASCII_WHITESPACE.contains(unit)) {
        data.push(u8::try_from(*unit).map_err(|_| ATOB_INVALID_CHARACTER)?);
    }
    if data.len() % BASE64_QUANTUM == 0 {
        for _ in 0..2 {
            if data.last() == Some(&BASE64_PADDING) {
                data.pop();
            }
        }
    }
    if !data.iter().all(|byte| codec::is_standard_base64_digit(*byte)) {
        return Err(ATOB_INVALID_CHARACTER);
    }
    if data.len() % BASE64_QUANTUM == 1 {
        return Err(ATOB_REMAINDER);
    }
    Ok(codec::base64_decode(&data))
}

/// `atob(string)`: the decoded latin1 string, or a negative failure result.
pub(crate) fn atob(c: &mut Context<'_>, _: RootId, args: &[RootId]) -> Result<RootId, RootedError> {
    let string = arg(c, args, 0);
    let units = string_units(c, string)?;
    Ok(match forgiving_base64(&units) {
        Ok(bytes) => {
            let units: Vec<u16> = bytes.into_iter().map(u16::from).collect();
            c.string_units_rooted(&units)
        }
        Err(code) => c.number(code),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn index_of_follows_node_clamping() {
        let haystack = b"abcabc";
        assert_eq!(index_of(haystack, b"bc", 0, 6, true, 1, Needle::String), 1.0);
        assert_eq!(index_of(haystack, b"bc", 2, 6, true, 1, Needle::String), 4.0);
        assert_eq!(index_of(haystack, b"bc", -1, 6, false, 1, Needle::String), 4.0);
        assert_eq!(index_of(haystack, b"bc", 3, 6, false, 1, Needle::String), 1.0);
        assert_eq!(index_of(haystack, b"bc", 0, 4, true, 1, Needle::String), 1.0);
        assert_eq!(index_of(haystack, b"bc", 2, 5, true, 1, Needle::String), -1.0);
        assert_eq!(index_of(haystack, b"", 10, 6, true, 1, Needle::String), 6.0);
        assert_eq!(index_of(b"a\0b\0a\0", b"a\0", 1, 6, true, UCS2_WIDTH, Needle::String), 4.0);
        assert_eq!(index_of(b"a\0b\0a\0", b"a\0", 1, 6, true, UCS2_WIDTH, Needle::Buffer), 0.0);
    }

    #[test]
    fn utf8_writes_stop_at_character_boundaries() {
        let encoded = "a€".as_bytes();
        assert_eq!(writable_prefix(encoded, "utf8", 3), 1);
        assert_eq!(writable_prefix(encoded, "utf8", 4), 4);
        assert_eq!(writable_prefix(&[0, 1, 2], "utf16le", 3), 2);
    }

    #[test]
    fn forgiving_base64_reports_node_failures() {
        let units = |text: &str| text.encode_utf16().collect::<Vec<_>>();
        assert_eq!(forgiving_base64(&units("YQ==")), Ok(b"a".to_vec()));
        assert_eq!(forgiving_base64(&units(" Y Q ")), Ok(b"a".to_vec()));
        assert_eq!(forgiving_base64(&units("a")), Err(ATOB_REMAINDER));
        assert_eq!(forgiving_base64(&units("a*")), Err(ATOB_INVALID_CHARACTER));
    }
}
