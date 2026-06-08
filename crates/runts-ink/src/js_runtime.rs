//! JavaScript global runtime functions for the compile path.
//!
//! These functions provide Rust implementations of standard JavaScript
//! globals so that generated Rust code can call them directly.

/// Encode a URI by escaping special characters.
/// Characters **not** encoded: `A-Z a-z 0-9 ; , / ? : @ & = + $ - _ . ! ~ * ' ( ) #`
#[allow(non_snake_case)]
pub fn encodeURI(s: impl AsRef<str>) -> String {
    encode_uri_internal(s.as_ref(), true)
}

/// Encode a URI component by escaping special characters.
/// Characters **not** encoded: `A-Z a-z 0-9 - _ . ! ~ * ' ( )`
#[allow(non_snake_case)]
pub fn encodeURIComponent(s: impl AsRef<str>) -> String {
    urlencoding::encode(s.as_ref()).into_owned()
}

/// Decode a URI previously created by `encodeURI`.
#[allow(non_snake_case)]
pub fn decodeURI(s: impl AsRef<str>) -> String {
    urlencoding::decode(s.as_ref())
        .map(|c| c.into_owned())
        .unwrap_or_else(|_| s.as_ref().to_string())
}

/// Decode a URI component previously created by `encodeURIComponent`.
#[allow(non_snake_case)]
pub fn decodeURIComponent(s: impl AsRef<str>) -> String {
    urlencoding::decode(s.as_ref())
        .map(|c| c.into_owned())
        .unwrap_or_else(|_| s.as_ref().to_string())
}

fn encode_uri_internal(s: &str, is_full_uri: bool) -> String {
    let mut result = String::with_capacity(s.len());
    for &b in s.as_bytes() {
        if should_encode_uri_byte(b, is_full_uri) {
            result.push_str(&format!("%{:02X}", b));
        } else {
            result.push(b as char);
        }
    }
    result
}

fn should_encode_uri_byte(b: u8, is_full_uri: bool) -> bool {
    const UNRESERVED: &[u8] = b"-_.!~*'()";
    if UNRESERVED.contains(&b) || b.is_ascii_alphanumeric() {
        return false;
    }
    if is_full_uri {
        const RESERVED: &[u8] = b";/?:@&=+$#,";
        if RESERVED.contains(&b) {
            return false;
        }
    }
    true
}
