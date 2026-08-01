//! WTF-8 string helpers for JavaScript strings containing lone surrogates.
//!
//! JavaScript strings are sequences of 16-bit code units (like UTF-16), meaning
//! lone surrogates (U+D800..U+DFFF) are valid code units.  The oxc parser
//! cannot store them as Rust `char` values (surrogates are not Unicode scalar
//! values), so it encodes them as `\u{FFFD}XXXX` in the decoded value — the
//! replacement character U+FFFD followed by the 4-hex-digit code point
//! (e.g. `\u{FFFD}d801` for U+D801).
//!
//! This module provides functions that scan a Rust `&str` for these encoded
//! sequences and handle them correctly for JS string iteration, length
//! computation, and index access.

use crate::value::Value;

/// oxc 0.142 encodes lone surrogates as U+FFFD followed by 4 hex digits.
/// U+FFFD in UTF-8 is 0xEF 0xBF 0xBD.
const U_FFFD_BYTES: [u8; 3] = [0xEF, 0xBF, 0xBD];

/// Check if `bytes[i..]` starts with the U+FFFD signature followed by 4 hex
/// digits (oxc's encoding for lone surrogates).
/// Returns `Some((code_point, consumed))` or `None`.
fn try_decode_escape(bytes: &[u8], i: usize) -> Option<(u32, usize)> {
    // oxc 0.142 format: U+FFFD (3 UTF-8 bytes) followed by 4 hex digits
    if i + 6 < bytes.len()
        && bytes[i] == U_FFFD_BYTES[0]
        && bytes[i + 1] == U_FFFD_BYTES[1]
        && bytes[i + 2] == U_FFFD_BYTES[2]
    {
        let hex_slice = &bytes[i + 3..i + 7];
        if hex_slice.iter().all(|b| b.is_ascii_hexdigit()) {
            let code_point =
                u32::from_str_radix(std::str::from_utf8(hex_slice).unwrap(), 16).ok()?;
            if (0xD800..=0xDFFF).contains(&code_point) || code_point <= 0x10FFFF {
                return Some((code_point, 7)); // 3 + 4 = 7 bytes consumed
            }
        }
    }
    None
}

/// Count UTF-16 code units in a string that may contain lone surrogate
/// encodings (produced by oxc parser).
///
/// Each non-surrogate BMP character = 1 code unit.
/// Each lone surrogate = 1 code unit.
/// Each astral character (surrogate pair) = 2 code units.
pub fn wtf8_utf16_count(s: &str) -> usize {
    let bytes = s.as_bytes();
    let mut count = 0;
    let mut i = 0;
    while i < bytes.len() {
        if let Some((cp, consumed)) = try_decode_escape(bytes, i) {
            if cp > 0xFFFF {
                // Astral: surrogate pair = 2 code units
                count += 2;
            } else {
                // BMP (including surrogates): 1 code unit
                count += 1;
            }
            i += consumed;
            continue;
        }
        let b = bytes[i];
        if b & 0x80 == 0 {
            // ASCII
            i += 1;
        } else if b & 0xE0 == 0xC0 {
            // 2-byte UTF-8 sequence
            i += 2;
        } else if b & 0xF0 == 0xE0 {
            // 3-byte UTF-8 sequence: 1 BMP code unit
            count += 1;
            i += 3;
            continue;
        } else if b & 0xF8 == 0xF0 {
            // 4-byte UTF-8 sequence: astral = 2 code units (surrogate pair)
            count += 2;
            i += 4;
            continue;
        } else {
            i += 1;
            continue;
        }
        // ASCII or 2-byte: 1 code unit
        count += 1;
    }
    count
}

/// Iterate over a string that may contain lone surrogate encodings (produced
/// by oxc parser), yielding each visible code unit.
///
/// For lone surrogates (stored as `\u{FFFD}D800`..`\u{FFFD}DFFF` by oxc),
/// yields the same literal encoding text so it matches how the parser stores
/// the same literal in user code.  For valid UTF-8 characters, yields the
/// character as-is.  For surrogate pairs (two encoded sequences for an astral
/// code point), yields the combined code point.
pub fn wtf8_for_of_iterate(s: &str) -> Vec<Value> {
    let bytes = s.as_bytes();
    let mut result = Vec::new();
    let mut i = 0;
    while i < bytes.len() {
        // Check for oxc-encoded surrogate
        if let Some((cp, consumed)) = try_decode_escape(bytes, i) {
            // Check if this is a high surrogate (U+D800..U+DBFF) followed by
            // a low surrogate escape (U+DC00..U+DFFF) — a surrogate pair.
            if (0xD800..=0xDBFF).contains(&cp) {
                let next_i = i + consumed;
                if let Some((low_cp, _)) = try_decode_escape(bytes, next_i) {
                    if (0xDC00..=0xDFFF).contains(&low_cp) {
                        let encoded = bytes[i..next_i + 7].to_vec();
                        result.push(Value::String(unsafe {
                            String::from_utf8_unchecked(encoded)
                        }));
                        i = next_i + 7; // second escape also 7 bytes
                        continue;
                    }
                }
            }
            // Lone surrogate or BMP code point: yield the same literal text
            // that oxc produced.  This matches how oxc stores a JS source
            // literal like '\ud801' — as the same encoding.
            result.push(Value::String(
                std::str::from_utf8(&bytes[i..i + consumed])
                    .unwrap_or("")
                    .to_string(),
            ));
            i += consumed;
            continue;
        }

        // Regular UTF-8 character
        let b = bytes[i];
        let char_len: usize = if b & 0x80 == 0 {
            1
        } else if b & 0xE0 == 0xC0 {
            2
        } else if b & 0xF0 == 0xE0 {
            3
        } else if b & 0xF8 == 0xF0 {
            4
        } else {
            1 // Invalid byte, skip
        };
        let end = (i + char_len).min(bytes.len());
        result.push(Value::String(unsafe {
            String::from_utf8_unchecked(bytes[i..end].to_vec())
        }));
        i = end;
    }
    result
}

/// Convert a string index (code unit offset) to the corresponding `Value`.
/// This is used for `str[i]` access where the string may contain lone
/// surrogate encodings.
pub fn wtf8_nth(s: &str, n: u32) -> Option<Value> {
    let items = wtf8_for_of_iterate(s);
    items.into_iter().nth(n as usize)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_utf16_count_ascii() {
        assert_eq!(wtf8_utf16_count("hello"), 5);
    }

    #[test]
    fn test_utf16_count_lone_surrogates() {
        // 'a' + \ud801 + 'b' + \ud801 - oxc 0.142 encodes as U+FFFD + "d801"
        let s = concat!("a", "\u{FFFD}", "d801", "b", "\u{FFFD}", "d801");
        assert_eq!(wtf8_utf16_count(s), 4);
    }

    #[test]
    fn test_utf16_count_astral() {
        // U+1F600 (😀) encoded as surrogate pair \uD83D\uDE00
        let s = concat!("\u{FFFD}", "d83d", "\u{FFFD}", "de00");
        assert_eq!(wtf8_utf16_count(s), 2);
    }

    #[test]
    fn test_for_of_iterate_lone_surrogates() {
        // 'a' + \ud801 + 'b' + \ud801
        let s = concat!("a", "\u{FFFD}", "d801", "b", "\u{FFFD}", "d801");
        let chars = wtf8_for_of_iterate(s);
        assert_eq!(chars.len(), 4);
        assert_eq!(chars[0], Value::String("a".to_string()));
        // The surrogate at index 1: yields the oxc encoding text (U+FFFD + "d801")
        let expected: String = "\u{FFFD}d801".into();
        assert_eq!(chars[1], Value::String(expected));
        assert_eq!(chars[2], Value::String("b".to_string()));
        let expected4: String = "\u{FFFD}d801".into();
        assert_eq!(chars[3], Value::String(expected4));
    }

    #[test]
    fn test_for_of_iterate_astral_pair() {
        // U+1F600 😀 as surrogate pair
        let s = concat!("\u{FFFD}", "d83d", "\u{FFFD}", "de00");
        let chars = wtf8_for_of_iterate(s);
        assert_eq!(chars.len(), 1);
        // Should produce the 4-byte UTF-8 encoding of U+1F600
        assert_eq!(chars[0].to_string(), "😀");
    }

    #[test]
    fn test_for_of_iterate_mixed() {
        // 'a' + U+1F600 (😀) + 'b'
        let s = concat!("a", "\u{FFFD}", "d83d", "\u{FFFD}", "de00", "b");
        let chars = wtf8_for_of_iterate(s);
        assert_eq!(chars.len(), 3);
        assert_eq!(chars[0], Value::String("a".to_string()));
        assert_eq!(chars[2], Value::String("b".to_string()));
    }

    #[test]
    fn test_for_of_iterate_standalone_ufffd() {
        // Standalone U+FFFD (replacement character) NOT followed by hex digits
        let s = concat!("a", "\u{FFFD}", "x");
        let chars = wtf8_for_of_iterate(s);
        assert_eq!(chars.len(), 3);
        assert_eq!(chars[0], Value::String("a".to_string()));
        assert_eq!(chars[1], Value::String("\u{FFFD}".to_string()));
        assert_eq!(chars[2], Value::String("x".to_string()));
    }
}
