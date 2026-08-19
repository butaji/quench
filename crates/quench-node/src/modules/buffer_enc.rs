//! Buffer string codecs — one canonical implementation of Node's
//! encoding names, byte encoders, and decoders. All Buffer entry
//! points (`from`, `toString`, `write`, `byteLength`) delegate here.

use quench_runtime::execute::VmError;
use quench_runtime::value::Value;

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

/// `ERR_BUFFER_OUT_OF_BOUNDS` coded `RangeError`.
pub fn buffer_out_of_bounds(message: &str) -> VmError {
    coded_error("RangeError", "ERR_BUFFER_OUT_OF_BOUNDS", message)
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
        _ => String::from_utf16_lossy(units).into_bytes(),
    }
}

/// Decode bytes to a string under a canonical encoding.
pub fn decode_str(bytes: &[u8], encoding: &str) -> Value {
    match encoding {
        "hex" => Value::String(bytes.iter().map(|b| format!("{b:02x}")).collect()),
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
        Err(_) => Value::StringUnits(std::rc::Rc::new(units)),
    }
}

const B64: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";

/// Base64 encode; `padding` controls trailing `=`, `url` selects the
/// URL-safe alphabet.
pub fn base64_encode(bytes: &[u8], padding: bool, url: bool) -> String {
    let mut out = String::new();
    for chunk in bytes.chunks(3) {
        let n =
            chunk.iter().fold(0u32, |acc, b| (acc << 8) | u32::from(*b)) << (8 * (3 - chunk.len()));
        for (shift, index) in [(18, 0), (12, 1), (6, 2), (0, 3)] {
            if index > chunk.len() {
                if padding {
                    out.push('=');
                }
            } else {
                let mut c = B64[((n >> shift) & 0x3F) as usize];
                if url {
                    c = match c {
                        b'+' => b'-',
                        b'/' => b'_',
                        other => other,
                    };
                }
                out.push(c as char);
            }
        }
    }
    out
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
pub fn is_utf8(value: &Value) -> bool {
    match value {
        Value::String(_) => true,
        Value::StringUnits(_) => false,
        Value::Uint8Array(view) => {
            let bytes = view.buffer.bytes.borrow();
            std::str::from_utf8(&bytes[view.byte_offset..view.byte_offset + view.length]).is_ok()
        }
        Value::ArrayBuffer(buf) => std::str::from_utf8(&buf.bytes.borrow()).is_ok(),
        _ => false,
    }
}

/// `buffer.isAscii` — every byte (or code unit) below 0x80.
pub fn is_ascii(value: &Value) -> bool {
    match value {
        Value::String(s) => s.is_ascii(),
        Value::StringUnits(units) => units.iter().all(|u| *u < 0x80),
        Value::Uint8Array(view) => {
            let bytes = view.buffer.bytes.borrow();
            bytes[view.byte_offset..view.byte_offset + view.length].is_ascii()
        }
        Value::ArrayBuffer(buf) => buf.bytes.borrow().is_ascii(),
        _ => false,
    }
}
