//! Buffer string codecs — one canonical implementation of Node's
//! encoding names, byte encoders, and decoders. All Buffer entry
//! points (`from`, `toString`, `write`, `byteLength`) delegate here.

use quench_runtime::execute::VmError;
use quench_runtime::value::Value;

use crate::modules::util::inspect;

/// Canonical encoding names accepted by `Buffer.isEncoding`.
pub fn canonical_encoding(name: &str) -> Option<&'static str> {
    match name.to_lowercase().as_str() {
        "utf8" | "utf-8" => Some("utf8"),
        "ucs2" | "ucs-2" | "utf16le" | "utf-16le" => Some("utf16le"),
        "latin1" | "binary" => Some("latin1"),
        "ascii" => Some("ascii"),
        "hex" => Some("hex"),
        "base64" => Some("base64"),
        "base64url" => Some("base64url"),
        _ => None,
    }
}
/// `ERR_UNKNOWN_ENCODING` coded `TypeError`.
pub fn unknown_encoding(name: &str) -> VmError {
    coded_error(
        "TypeError",
        "ERR_UNKNOWN_ENCODING",
        &format!("Unknown encoding: {name}"),
    )
}

/// `ERR_OUT_OF_RANGE` coded `RangeError`, Node's message shape.
pub fn out_of_range(name: &str, range: &str, received: &str) -> VmError {
    coded_error(
        "RangeError",
        "ERR_OUT_OF_RANGE",
        &format!(
            "The value of \"{name}\" is out of range. It must be {range}. Received {received}"
        ),
    )
}

/// `ERR_INVALID_ARG_TYPE` coded `TypeError`, Node's message shape.
pub fn invalid_arg_type(message: String) -> VmError {
    coded_error("TypeError", "ERR_INVALID_ARG_TYPE", &message)
}

/// `ERR_INVALID_ARG_VALUE` coded `TypeError`.
pub fn invalid_arg_value(message: String) -> VmError {
    coded_error("TypeError", "ERR_INVALID_ARG_VALUE", &message)
}

pub fn invalid_state(message: String) -> VmError {
    coded_error("Error", "ERR_INVALID_STATE", &message)
}

pub fn invalid_this() -> VmError {
    coded_error(
        "TypeError",
        "ERR_INVALID_THIS",
        "Cannot call StringDecoder method on an incompatible receiver",
    )
}

/// `ERR_BUFFER_OUT_OF_BOUNDS` coded `RangeError`.
pub fn buffer_out_of_bounds(message: &str) -> VmError {
    coded_error("RangeError", "ERR_BUFFER_OUT_OF_BOUNDS", message)
}

pub fn string_too_long() -> VmError {
    let error = quench_runtime::builtins::error(
        quench_runtime::ops::Builtin::Error,
        &[Value::String(
            "Cannot create a string longer than the maximum allowed length".into(),
        )],
    );
    let error = quench_runtime::execute::set_property(
        error,
        "code",
        Value::String("ERR_STRING_TOO_LONG".into()),
    );
    VmError::Thrown(error)
}

fn coded_error(name: &str, code: &str, message: &str) -> VmError {
    let builtin = match name {
        "RangeError" => quench_runtime::ops::Builtin::RangeError,
        _ => quench_runtime::ops::Builtin::TypeError,
    };
    let error = quench_runtime::builtins::error(builtin, &[Value::String(message.to_string())]);
    let error =
        quench_runtime::execute::set_property(error, "code", Value::String(code.to_string()));
    VmError::Thrown(error)
}

/// Format a number the way Node prints it in error messages.
pub fn fmt_num(n: f64) -> String {
    if n.is_nan() {
        "NaN".to_string()
    } else if n.is_infinite() {
        if n < 0.0 {
            "-Infinity".to_string()
        } else {
            "Infinity".to_string()
        }
    } else if n.fract() == 0.0 && n.abs() > 4_294_967_296.0 {
        separated(&format!("{n:.0}"))
    } else if n.fract() == 0.0 && n.abs() < 1e15 {
        format!("{n:.0}")
    } else {
        format!("{n}")
    }
}

/// Node's `addNumericalSeparator`: group digits with underscores.
pub fn separated(digits: &str) -> String {
    let (sign, digits) = digits.strip_prefix('-').map_or(("", digits), |d| ("-", d));
    let mut out = String::new();
    for (i, c) in digits.chars().enumerate() {
        if i > 0 && (digits.len() - i) % 3 == 0 {
            out.push('_');
        }
        out.push(c);
    }
    format!("{sign}{out}")
}

/// Encode a string value to bytes under a canonical encoding.
pub fn encode_value(value: &Value, encoding: &str) -> Result<Vec<u8>, VmError> {
    match value {
        Value::String(s) => Ok(encode_str(s, encoding)),
        Value::StringUnits(units) => Ok(encode_units(units, encoding)),
        _ => Err(invalid_arg_type(format!(
            "The \"string\" argument must be of type string.{}",
            crate::modules::util::invalid_arg_received(value)
        ))),
    }
}

/// Encode a UTF-8 string to bytes under a canonical encoding.
pub fn encode_str(input: &str, encoding: &str) -> Vec<u8> {
    match encoding {
        "hex" => hex_decode(input.as_bytes()),
        "base64" | "base64url" => base64_decode(input.as_bytes()),
        "latin1" | "ascii" => input.chars().map(|c| c as u32 as u8).collect(),
        "utf16le" => input.encode_utf16().flat_map(u16::to_le_bytes).collect(),
        _ => input.as_bytes().to_vec(),
    }
}

/// Encode raw UTF-16 code units (lone surrogates become U+FFFD for utf8).
fn encode_units(units: &[u16], encoding: &str) -> Vec<u8> {
    match encoding {
        "utf16le" => units.iter().flat_map(|u| u.to_le_bytes()).collect(),
        "latin1" | "ascii" => units.iter().map(|u| *u as u8).collect(),
        "hex" => hex_decode(&String::from_utf16_lossy(units).into_bytes()),
        "base64" | "base64url" => base64_decode(&String::from_utf16_lossy(units).into_bytes()),
        _ => utf8_units(units),
    }
}

fn utf8_units(units: &[u16]) -> Vec<u8> {
    // Encode directly from the canonical UTF-16 representation.  Going
    // through `String::from_utf16_lossy` first materializes a second UTF-8
    // string and then copies it into a byte vector; Buffer callers commonly
    // need exactly those bytes, so keep the semantic replacement of lone
    // surrogates while writing one representation.
    // Three bytes is the common upper bound per UTF-16 unit (a surrogate
    // pair is four bytes for two units), avoiding repeated growth for BMP and
    // replacement-heavy inputs while keeping the allocation bounded.
    let mut out = Vec::with_capacity(units.len().saturating_mul(3));
    let mut index = 0;
    while index < units.len() {
        let unit = units[index];
        let code_point = if (0xD800..=0xDBFF).contains(&unit) {
            if let Some(&next) = units.get(index + 1) {
                if (0xDC00..=0xDFFF).contains(&next) {
                    index += 1;
                    0x10000 + ((u32::from(unit) - 0xD800) << 10)
                        + (u32::from(next) - 0xDC00)
                } else {
                    0xFFFD
                }
            } else {
                0xFFFD
            }
        } else if (0xDC00..=0xDFFF).contains(&unit) {
            0xFFFD
        } else {
            u32::from(unit)
        };

        match code_point {
            0..=0x7F => out.push(code_point as u8),
            0x80..=0x7FF => {
                out.push((0xC0 | (code_point >> 6)) as u8);
                out.push((0x80 | (code_point & 0x3F)) as u8);
            }
            0x800..=0xFFFF => {
                out.push((0xE0 | (code_point >> 12)) as u8);
                out.push((0x80 | ((code_point >> 6) & 0x3F)) as u8);
                out.push((0x80 | (code_point & 0x3F)) as u8);
            }
            _ => {
                out.push((0xF0 | (code_point >> 18)) as u8);
                out.push((0x80 | ((code_point >> 12) & 0x3F)) as u8);
                out.push((0x80 | ((code_point >> 6) & 0x3F)) as u8);
                out.push((0x80 | (code_point & 0x3F)) as u8);
            }
        }
        index += 1;
    }
    out
}

/// Decode bytes to a string under a canonical encoding.
pub fn decode_str(bytes: &[u8], encoding: &str) -> Value {
    match encoding {
        "hex" => Value::String(hex::encode(bytes)),
        "latin1" => Value::String(bytes.iter().map(|b| *b as char).collect()),
        "ascii" => Value::String(bytes.iter().map(|b| (b & 0x7F) as char).collect()),
        "base64" => Value::String(base64_encode(bytes, true, false)),
        "base64url" => Value::String(base64_encode(bytes, false, true)),
        "utf16le" => decode_utf16le(bytes),
        _ => Value::String(String::from_utf8_lossy(bytes).into_owned()),
    }
}

fn decode_utf16le(bytes: &[u8]) -> Value {
    let units: Vec<u16> = bytes
        .chunks_exact(2)
        .map(|pair| u16::from_le_bytes([pair[0], pair[1]]))
        .collect();
    match String::from_utf16(&units) {
        Ok(s) => Value::String(s),
        Err(_) => Value::StringUnits(std::rc::Rc::new(
            quench_runtime::value::StringUnitsData::new(units),
        )),
    }
}

const B64: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";

/// Base64 encode; `padding` controls trailing `=`, `url` selects the
/// URL-safe alphabet. Backed by the `base64` crate (exact byte fidelity).
pub fn base64_encode(bytes: &[u8], padding: bool, url: bool) -> String {
    use base64::Engine;
    let engine = if url {
        if padding {
            base64::engine::general_purpose::URL_SAFE
        } else {
            base64::engine::general_purpose::URL_SAFE_NO_PAD
        }
    } else if padding {
        base64::engine::general_purpose::STANDARD
    } else {
        base64::engine::general_purpose::STANDARD_NO_PAD
    };
    engine.encode(bytes)
}

/// Base64 decode: ignores whitespace and foreign bytes, accepts both
/// alphabets, stops at `=` (Node's forgiving decoder).
pub fn base64_decode(input: &[u8]) -> Vec<u8> {
    let mut out = Vec::new();
    let mut acc = 0u32;
    let mut bits = 0u32;
    for &byte in input {
        if byte == b'=' {
            break;
        }
        let mapped = match byte {
            b'-' => b'+',
            b'_' => b'/',
            other => other,
        };
        let Some(digit) = B64.iter().position(|c| *c == mapped) else {
            continue;
        };
        acc = (acc << 6) | digit as u32;
        bits += 6;
        if bits >= 8 {
            bits -= 8;
            out.push((acc >> bits) as u8);
        }
    }
    out
}

/// Hex decode: stops at the first invalid pair; a trailing half byte
/// is dropped (Node's truncation semantics).
pub fn hex_decode(input: &[u8]) -> Vec<u8> {
    let mut out = Vec::new();
    for pair in input.chunks(2) {
        if pair.len() < 2 {
            break;
        }
        let (Some(hi), Some(lo)) = (hex_digit(pair[0]), hex_digit(pair[1])) else {
            break;
        };
        out.push((hi << 4) | lo);
    }
    out
}

fn hex_digit(byte: u8) -> Option<u8> {
    match byte {
        b'0'..=b'9' => Some(byte - b'0'),
        b'a'..=b'f' => Some(byte - b'a' + 10),
        b'A'..=b'F' => Some(byte - b'A' + 10),
        _ => None,
    }
}

/// `buffer.isUtf8` — strict UTF-8 validation of the input.
pub fn is_utf8(value: &Value) -> Result<bool, VmError> {
    match value {
        Value::String(_) | Value::StringUnits(_) => Err(invalid_arg_type(
            "The \"value\" argument must be an instance of ArrayBuffer or ArrayBufferView".into(),
        )),
        Value::ArrayBuffer(buf) => {
            if *buf.detached.borrow() {
                return Ok(true);
            }
            Ok(std::str::from_utf8(&buf.bytes.borrow()).is_ok())
        }
        value if is_view(value) => {
            view_bytes(value).map_or(Ok(true), |(buffer, offset, length)| {
                if *buffer.detached.borrow() {
                    return Ok(true);
                }
                let bytes = buffer.bytes.borrow();
                Ok(std::str::from_utf8(&bytes[offset..offset + length]).is_ok())
            })
        }
        _ => Err(invalid_arg_type(
            "The \"value\" argument must be an instance of ArrayBuffer or ArrayBufferView".into(),
        )),
    }
}

/// `buffer.isAscii` — every byte (or code unit) below 0x80.
pub fn is_ascii(value: &Value) -> Result<bool, VmError> {
    match value {
        Value::String(_) | Value::StringUnits(_) => Err(invalid_arg_type(
            "The \"value\" argument must be an instance of ArrayBuffer or ArrayBufferView".into(),
        )),
        Value::ArrayBuffer(buf) => {
            if *buf.detached.borrow() {
                return Ok(true);
            }
            Ok(buf.bytes.borrow().is_ascii())
        }
        value if is_view(value) => {
            view_bytes(value).map_or(Ok(true), |(buffer, offset, length)| {
                if *buffer.detached.borrow() {
                    return Ok(true);
                }
                let bytes = buffer.bytes.borrow();
                Ok(bytes[offset..offset + length].is_ascii())
            })
        }
        _ => Err(invalid_arg_type(
            "The \"value\" argument must be an instance of ArrayBuffer or ArrayBufferView".into(),
        )),
    }
}

fn is_view(value: &Value) -> bool {
    matches!(
        value,
        Value::Float64Array(_)
            | Value::Float32Array(_)
            | Value::Int8Array(_)
            | Value::Int16Array(_)
            | Value::Int32Array(_)
            | Value::BigInt64Array(_)
            | Value::BigUint64Array(_)
            | Value::Uint32Array(_)
            | Value::Uint8Array(_)
            | Value::Uint8ClampedArray(_)
            | Value::Uint16Array(_)
            | Value::DataView(_)
    )
}

macro_rules! typed_view_fact {
    ($value:expr, $( $variant:ident ),+ $(,)?) => {
        match $value {
            $(Value::$variant(view) => Some((view.buffer.clone(), view.byte_offset, view.byte_length())),)+
            Value::DataView(view) => Some((view.buffer.clone(), view.byte_offset, view.byte_length())),
            _ => None,
        }
    };
}

fn view_bytes(
    value: &Value,
) -> Option<(
    std::rc::Rc<quench_runtime::value::ArrayBufferData>,
    usize,
    usize,
)> {
    typed_view_fact!(
        value,
        Float64Array,
        Float32Array,
        Int8Array,
        Int16Array,
        Int32Array,
        BigInt64Array,
        BigUint64Array,
        Uint32Array,
        Uint8Array,
        Uint8ClampedArray,
        Uint16Array,
    )
}

/// Node's `ERR_INVALID_ARG_TYPE` "Received …" suffix.
pub fn invalid_arg_received(value: &Value) -> String {
    match value {
        Value::Null => " Received null".into(),
        Value::Undefined => " Received undefined".into(),
        Value::Object(_) | Value::Proxy(_) => invalid_arg_object(value),
        Value::Function(_) | Value::BoundFunction(_) => {
            let name = quench_runtime::execute::get_property(value, "name");
            match name {
                Value::String(name) => format!(" Received function {name}"),
                _ => " Received function".into(),
            }
        }
        Value::Array(_) => " Received an instance of Array".into(),
        Value::Uint8Array(_) if crate::modules::buffer::is_buffer(std::slice::from_ref(value)) => {
            " Received an instance of Buffer".into()
        }
        Value::Boolean(_) => format!(" Received type boolean ({})", inspect(value)),
        Value::Number(_) | Value::BigInt(_) => format!(
            " Received type {} ({})",
            if matches!(value, Value::Number(_)) {
                "number"
            } else {
                "bigint"
            },
            inspect(value)
        ),
        Value::String(s) if s.starts_with("Symbol.") || s.starts_with("Symbol.for.") => {
            format!(" Received type symbol ({})", inspect(value))
        }
        Value::String(_) => format!(" Received type string ({})", inspect(value)),
        _ => format!(" Received {}", inspect(value)),
    }
}

fn invalid_arg_object(value: &Value) -> String {
    let name = quench_runtime::execute::get_property(value, "constructor");
    let name = quench_runtime::execute::get_property(&name, "name");
    match name {
        Value::String(name) if !name.is_empty() => format!(" Received an instance of {name}"),
        _ if matches!(
            quench_runtime::execute::get_prototype_of(value),
            Ok(Value::Null)
        ) =>
        {
            " Received [Object: null prototype] {}".into()
        }
        _ => " Received an instance of Object".into(),
    }
}
