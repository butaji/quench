//! WTF-8 string helpers for JavaScript strings containing lone surrogates.
//!
//! JavaScript strings are sequences of 16-bit code units (like UTF-16), meaning
//! lone surrogates (U+D800..U+DFFF) are valid code units.  The oxc parser
//! cannot store them as Rust `char` values (surrogates are not Unicode scalar
//! values), so it writes them as the literal 6-character escape sequence
//! `\uXXXX` (e.g. `\ud801`).
//!
//! This module provides functions that scan a Rust `&str` for these `\uXXXX`
//! sequences and handle them correctly for JS string iteration, length
//! computation, and index access.

use crate::value::Value;

/// Decode a `\uXXXX` escape at position `i` in `bytes`.
/// Returns `(code_point, consumed)` where `consumed` is the number of bytes
/// consumed (6 for `\uXXXX`).  Returns `None` if `bytes[i..]` doesn't start
/// with a valid `\uXXXX` sequence.
fn try_decode_escape(bytes: &[u8], i: usize) -> Option<(u32, usize)> {
    if i + 5 < bytes.len() && bytes[i] == b'\\' && bytes[i + 1] == b'u' {
        let hex_slice = &bytes[i + 2..i + 6];
        if hex_slice.iter().all(|b| b.is_ascii_hexdigit()) {
            let code_point =
                u32::from_str_radix(std::str::from_utf8(hex_slice).unwrap(), 16).ok()?;
            if code_point <= 0x10FFFF {
                return Some((code_point, 6));
            }
        }
    }
    None
}

/// Count UTF-16 code units in a string that may contain `\uXXXX` escape
/// sequences (used by oxc for lone surrogates).
///
/// Each non-surrogate BMP character = 1 code unit.
/// Each `\uXXXX` escape that decodes to a surrogate = 1 code unit.
/// Each astral character (pair of `\uXXXX\uXXXX` or a 4-byte UTF-8 sequence)
/// = 2 code units (surrogate pair).
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

/// Iterate over a string that may contain `\uXXXX` escape sequences,
/// yielding each visible code unit.  For lone surrogates (stored as
/// `\uD800`..`\uDFFF` by oxc), yields the surrogate as a single-character
/// string.  For valid UTF-8 characters, yields the character as-is.
/// For surrogate pairs (two `\uXXXX\uXXXX` sequences for an astral code
/// point), yields the combined code point.
pub fn wtf8_for_of_iterate(s: &str) -> Vec<Value> {
    let bytes = s.as_bytes();
    let mut result = Vec::new();
    let mut i = 0;
    while i < bytes.len() {
        // Check for \uXXXX escape sequence
        if let Some((mut cp, consumed)) = try_decode_escape(bytes, i) {
            // Check if this is a high surrogate (U+D800..U+DBFF) followed by
            // a low surrogate escape (U+DC00..U+DFFF) — ie. a surrogate pair.
            if (0xD800..=0xDBFF).contains(&cp) {
                let next_i = i + consumed;
                if let Some((low_cp, _)) = try_decode_escape(bytes, next_i) {
                    if (0xDC00..=0xDFFF).contains(&low_cp) {
                        // Valid surrogate pair — combine into astral code point
                        cp = 0x10000 + ((cp - 0xD800) << 10) + (low_cp - 0xDC00);
                        // Encode as 4-byte UTF-8 (valid - astral code points)
                        let encoded = encode_utf8(cp);
                        result.push(Value::String(
                            // SAFETY: astral code points produce valid UTF-8
                            unsafe { String::from_utf8_unchecked(encoded) },
                        ));
                        i = next_i + 6;
                        continue;
                    }
                }
            }
            // Lone surrogate: oxc stores it as the literal 6-char escape
            // sequence `\uXXXX`.  We yield this literal substring so it
            // matches `var x = '\\uXXXX'` which oxc also stores literally.
            // Normal BMP code points: also yield the original substring
            // from the source (it's valid UTF-8 and matches any literal
            // in user code that uses the same escape).
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

/// Encode a Unicode code point to 1-4 byte UTF-8 sequence.
fn encode_utf8(cp: u32) -> Vec<u8> {
    if cp <= 0x7F {
        vec![cp as u8]
    } else if cp <= 0x7FF {
        vec![0xC0 | (cp >> 6) as u8, 0x80 | (cp & 0x3F) as u8]
    } else if cp <= 0xFFFF {
        vec![
            0xE0 | (cp >> 12) as u8,
            0x80 | ((cp >> 6) & 0x3F) as u8,
            0x80 | (cp & 0x3F) as u8,
        ]
    } else {
        vec![
            0xF0 | (cp >> 18) as u8,
            0x80 | ((cp >> 12) & 0x3F) as u8,
            0x80 | ((cp >> 6) & 0x3F) as u8,
            0x80 | (cp & 0x3F) as u8,
        ]
    }
}

/// Convert a string index (code unit offset) to the corresponding `Value`.
/// This is used for `str[i]` access where the string may contain `\uXXXX`
/// patterns for lone surrogates.
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
    fn test_utf16_count_surrogates() {
        // 'a' + \ud801 + 'b' + \ud801
        let s = "a\\ud801b\\ud801";
        assert_eq!(wtf8_utf16_count(s), 4);
    }

    #[test]
    fn test_utf16_count_astral() {
        // U+1F600 (😀) encoded as surrogate pair \uD83D\uDE00
        let s = "\\ud83d\\ude00";
        assert_eq!(wtf8_utf16_count(s), 2);
    }

    #[test]
    fn test_for_of_iterate_surrogates() {
        let s = "a\\ud801b\\ud801";
        let chars = wtf8_for_of_iterate(s);
        assert_eq!(chars.len(), 4);
        assert_eq!(chars[0], Value::String("a".to_string()));
        // The surrogate at index 1: should be the literal \uXXXX text
        assert_eq!(chars[1], Value::String("\\ud801".to_string()));
        assert_eq!(chars[2], Value::String("b".to_string()));
        assert_eq!(chars[3], Value::String("\\ud801".to_string()));
    }

    #[test]
    fn test_for_of_iterate_astral_pair() {
        // U+1F600 😀 as surrogate pair
        let s = "\\ud83d\\ude00";
        let chars = wtf8_for_of_iterate(s);
        assert_eq!(chars.len(), 1);
        // Should produce the 4-byte UTF-8 encoding of U+1F600
        assert_eq!(chars[0].to_string(), "😀");
    }

    #[test]
    fn test_for_of_iterate_mixed() {
        // 'a' + U+1F600 (😀) + 'b'
        let s = "a\\ud83d\\ude00b";
        let chars = wtf8_for_of_iterate(s);
        assert_eq!(chars.len(), 3);
        assert_eq!(chars[0], Value::String("a".to_string()));
        assert_eq!(chars[2], Value::String("b".to_string()));
    }
}
