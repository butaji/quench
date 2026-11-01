//! VM-independent byte codecs used by Node Buffer and string-decoder APIs.

/// Canonical encoding names accepted by `Buffer.isEncoding`.
pub(crate) fn canonical_encoding(name: &str) -> Option<&'static str> {
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

/// Encode a UTF-8 string to bytes under a canonical encoding.
pub(crate) fn encode_str(input: &str, encoding: &str) -> Vec<u8> {
    match encoding {
        "hex" => hex_decode(input.as_bytes()),
        "base64" | "base64url" => base64_decode(input.as_bytes()),
        "latin1" | "ascii" | "utf16le" => encode_fixed_width_units(input.encode_utf16(), encoding),
        _ => input.as_bytes().to_vec(),
    }
}

/// Encode JavaScript's original UTF-16 units without dropping lone surrogates.
pub(crate) fn encode_units(input: &[u16], encoding: &str) -> Vec<u8> {
    match encoding {
        "latin1" | "ascii" | "utf16le" => encode_fixed_width_units(input.iter().copied(), encoding),
        "base64" | "base64url" => {
            base64_decode(&input.iter().map(|unit| *unit as u8).collect::<Vec<_>>())
        }
        _ => encode_str(&String::from_utf16_lossy(input), encoding),
    }
}

fn encode_fixed_width_units(input: impl Iterator<Item = u16>, encoding: &str) -> Vec<u8> {
    if encoding == "utf16le" {
        input.flat_map(u16::to_le_bytes).collect()
    } else {
        input.map(|unit| unit as u8).collect()
    }
}

/// Decode bytes to UTF-16 code units under a canonical encoding.
pub(crate) fn decode_units(bytes: &[u8], encoding: &str) -> Vec<u16> {
    decode_units_limited(bytes, encoding, usize::MAX).expect("unbounded codec output length")
}

/// Check the resulting string's UTF-16 length before allocating its unit storage.
/// The caller supplies the original runtime limit; public mutable exports are not read.
pub(crate) fn decode_units_limited(
    bytes: &[u8],
    encoding: &str,
    maximum: usize,
) -> Option<Vec<u16>> {
    const HEX_CHARACTERS_PER_BYTE: usize = 2;
    let direct_length = match encoding {
        "latin1" | "ascii" => Some(bytes.len()),
        "hex" => Some(bytes.len().checked_mul(HEX_CHARACTERS_PER_BYTE)?),
        "utf16le" => Some(bytes.len() / std::mem::size_of::<u16>()),
        "base64" | "base64url" => {
            // Each complete three-byte quantum becomes four characters.
            const INPUT_QUANTUM: usize = 3;
            const OUTPUT_QUANTUM: usize = 4;
            let complete = (bytes.len() / INPUT_QUANTUM).checked_mul(OUTPUT_QUANTUM)?;
            let remainder = bytes.len() % INPUT_QUANTUM;
            Some(complete.checked_add(if remainder == 0 {
                0
            } else if encoding == "base64" {
                OUTPUT_QUANTUM
            } else {
                remainder + 1
            })?)
        }
        _ => None,
    };
    if direct_length.is_some_and(|length| length > maximum) {
        return None;
    }
    let text = match encoding {
        "hex" => hex::encode(bytes),
        "latin1" => return Some(bytes.iter().map(|byte| u16::from(*byte)).collect()),
        "ascii" => return Some(bytes.iter().map(|byte| u16::from(byte & 0x7f)).collect()),
        "base64" => base64_encode(bytes, true, false),
        "base64url" => base64_encode(bytes, false, true),
        "utf16le" => {
            return Some(
                bytes
                    .chunks_exact(std::mem::size_of::<u16>())
                    .map(|pair| u16::from_le_bytes([pair[0], pair[1]]))
                    .collect(),
            );
        }
        _ => {
            let text = String::from_utf8_lossy(bytes);
            if bytes.len() > maximum && text.encode_utf16().count() > maximum {
                return None;
            }
            return Some(text.encode_utf16().collect());
        }
    };
    Some(text.encode_utf16().collect())
}

const B64: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";

pub(crate) fn is_standard_base64_digit(byte: u8) -> bool {
    B64.contains(&byte)
}

/// Base64 encode with Node's standard and URL-safe output variants.
pub(crate) fn base64_encode(bytes: &[u8], padding: bool, url: bool) -> String {
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
pub(crate) fn base64_decode(input: &[u8]) -> Vec<u8> {
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

/// Hex decode: drops a trailing half byte and stops at the first invalid pair.
pub(crate) fn hex_decode(input: &[u8]) -> Vec<u8> {
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
