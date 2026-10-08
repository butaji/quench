//! VM-independent byte codecs shared by both Node Buffer adapters.

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
        "latin1" | "ascii" => input.chars().map(|c| c as u32 as u8).collect(),
        "utf16le" => input.encode_utf16().flat_map(u16::to_le_bytes).collect(),
        _ => input.as_bytes().to_vec(),
    }
}

/// Encode raw UTF-16 code units (lone surrogates become U+FFFD for utf8).
pub(crate) fn encode_units(units: &[u16], encoding: &str) -> Vec<u8> {
    match encoding {
        "utf16le" => units.iter().flat_map(|u| u.to_le_bytes()).collect(),
        "latin1" | "ascii" => units.iter().map(|u| *u as u8).collect(),
        "hex" => hex_decode(&String::from_utf16_lossy(units).into_bytes()),
        "base64" | "base64url" => base64_decode(&String::from_utf16_lossy(units).into_bytes()),
        _ => utf8_units(units),
    }
}

fn utf8_units(units: &[u16]) -> Vec<u8> {
    // Encode directly from the canonical UTF-16 representation. Going
    // through `String::from_utf16_lossy` first materializes another string.
    let mut out = Vec::with_capacity(units.len().saturating_mul(3));
    let mut index = 0;
    while index < units.len() {
        let unit = units[index];
        let code_point = if (0xD800..=0xDBFF).contains(&unit) {
            if let Some(&next) = units.get(index + 1) {
                if (0xDC00..=0xDFFF).contains(&next) {
                    index += 1;
                    0x10000 + ((u32::from(unit) - 0xD800) << 10) + (u32::from(next) - 0xDC00)
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

/// Decode bytes to UTF-16 code units under a canonical encoding.
pub(crate) fn decode_units(bytes: &[u8], encoding: &str) -> Vec<u16> {
    let text = match encoding {
        "hex" => hex::encode(bytes),
        "latin1" => return bytes.iter().map(|byte| u16::from(*byte)).collect(),
        "ascii" => return bytes.iter().map(|byte| u16::from(byte & 0x7f)).collect(),
        "base64" => base64_encode(bytes, true, false),
        "base64url" => base64_encode(bytes, false, true),
        "utf16le" => {
            return bytes
                .chunks_exact(2)
                .map(|pair| u16::from_le_bytes([pair[0], pair[1]]))
                .collect();
        }
        _ => String::from_utf8_lossy(bytes).into_owned(),
    };
    text.encode_utf16().collect()
}

const B64: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";

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
