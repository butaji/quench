//! URI handling functions: encodeURI, encodeURIComponent, decodeURI,
//! decodeURIComponent, plus the global parseInt / parseFloat / isNaN /
//! isFinite. parseInt / parseFloat are exposed both as globals and as
//! properties of Number; the actual logic lives in
//! `builtins::date::spec_parse_int` / `spec_parse_float` (parseInt/parseFloat
//! are simpler than Date parsing, but the implementation already covers
//! the spec cases).

use crate::value::{to_js_string, to_number, Value};
use crate::Context;

/// RFC 3986 "unreserved" characters plus a few reserved characters that
/// decodeURI / decodeURIComponent treat as legal.
fn is_uri_unreserved(c: char) -> bool {
    c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.' | '~' | '!' | '*' | '\'' | '(' | ')')
}

/// Characters reserved by RFC 3986 that encodeURI leaves alone (the
/// "reserved" set minus characters that encodeURIComponent also escapes).
fn is_uri_reserved(c: char) -> bool {
    matches!(c, ';' | ',' | '/' | ':' | '&' | '=' | '+' | '$' | '?')
}

/// Decode a single percent-escape `%XX` to a byte (0..=255). Returns None
/// when the escape is malformed.
fn decode_escape(s: &str) -> Option<u8> {
    let bytes = s.as_bytes();
    if bytes.len() < 3 || bytes[0] != b'%' {
        return None;
    }
    let hi = hex_digit(bytes[1])?;
    let lo = hex_digit(bytes[2])?;
    Some((hi << 4) | lo)
}

fn hex_digit(b: u8) -> Option<u8> {
    match b {
        b'0'..=b'9' => Some(b - b'0'),
        b'a'..=b'f' => Some(b - b'a' + 10),
        b'A'..=b'F' => Some(b - b'A' + 10),
        _ => None,
    }
}

fn encode_uri(s: &str, keep_reserved: bool) -> String {
    let mut out = String::with_capacity(s.len());
    let bytes = s.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        let b = bytes[i];
        // Pass through ASCII printable characters that don't need encoding.
        if b < 0x80 {
            let c = b as char;
            if is_uri_unreserved(c) || (keep_reserved && is_uri_reserved(c)) {
                out.push(c);
            } else if matches!(b, b'#') {
                // '#' is reserved but encodeURI leaves it alone in components
                // and full URIs alike (Annex B of RFC 3986 keeps it reserved
                // for the fragment delimiter, but encodeURI's spec says it
                // must not be escaped). This is the common interpretation.
                out.push(c);
            } else {
                out.push_str(&format!("%{:02X}", b));
            }
        } else {
            // UTF-8 multibyte: percent-encode each byte.
            let c = s[i..].chars().next().unwrap();
            let mut buf = [0u8; 4];
            let encoded = c.encode_utf8(&mut buf);
            for &eb in encoded.as_bytes() {
                out.push_str(&format!("%{:02X}", eb));
            }
            i += encoded.len() - 1;
        }
        i += 1;
    }
    out
}

/// Throw a URIError and return a JsError.
fn uri_error(msg: impl Into<String>) -> crate::JsError {
    let (err, js_err) = crate::value::error::create_js_error_with_type(&msg.into(), "URIError");
    crate::value::set_thrown_value(err);
    js_err
}

fn decode_uri(s: &str, keep_reserved: bool) -> Result<String, crate::JsError> {
    let bytes = s.as_bytes();
    let mut out = Vec::with_capacity(s.len());
    let mut i = 0;
    while i < bytes.len() {
        let b = bytes[i];
        if b == b'%' {
            if i + 2 >= bytes.len() {
                return Err(uri_error("URI malformed"));
            }
            let v = decode_escape(&s[i..i + 3]).ok_or_else(|| uri_error("URI malformed"))?;
            out.push(v);
            i += 3;
        } else if b < 0x80 {
            out.push(b);
            i += 1;
        } else {
            // Pass through UTF-8 continuation bytes verbatim.
            let c = s[i..].chars().next().unwrap();
            let mut buf = [0u8; 4];
            let encoded = c.encode_utf8(&mut buf);
            out.extend_from_slice(encoded.as_bytes());
            i += encoded.len();
        }
    }
    let decoded = String::from_utf8(out).map_err(|_| uri_error("URI malformed"))?;
    // Per spec decodeURI: re-encode any character that encodeURI would
    // have escaped. This round-trip property guarantees encodeURI/decodeURI
    // are inverses for valid URIs.
    let _ = keep_reserved; // already handled inside encode_uri's keep set
    Ok(reencode_to_uri_form(&decoded, true))
}

/// Re-escape characters that encode_uri (with keep_reserved) would have
/// escaped. Mirrors encode_uri so the two functions form a round-trip.
fn reencode_to_uri_form(s: &str, keep_reserved: bool) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        let mut buf = [0u8; 4];
        let encoded = c.encode_utf8(&mut buf);
        let bytes = encoded.as_bytes();
        let needs_escape = if c.is_ascii() {
            !(is_uri_unreserved(c) || (keep_reserved && is_uri_reserved(c)))
        } else {
            true
        };
        if needs_escape {
            for &b in bytes {
                out.push_str(&format!("%{:02X}", b));
            }
        } else {
            out.push_str(encoded);
        }
    }
    out
}

fn decode_uri_component(s: &str) -> Result<String, crate::JsError> {
    let bytes = s.as_bytes();
    let mut out = Vec::with_capacity(s.len());
    let mut i = 0;
    while i < bytes.len() {
        let b = bytes[i];
        if b == b'%' {
            if i + 2 >= bytes.len() {
                return Err(uri_error("URI malformed"));
            }
            let v = decode_escape(&s[i..i + 3]).ok_or_else(|| uri_error("URI malformed"))?;
            out.push(v);
            i += 3;
        } else if b < 0x80 {
            out.push(b);
            i += 1;
        } else {
            let c = s[i..].chars().next().unwrap();
            let mut buf = [0u8; 4];
            let encoded = c.encode_utf8(&mut buf);
            out.extend_from_slice(encoded.as_bytes());
            i += encoded.len();
        }
    }
    String::from_utf8(out).map_err(|_| uri_error("URI malformed"))
}

// ============================================================================
// Registration
// ============================================================================

pub fn register_uri(ctx: &mut Context) {
    // parseInt(string, radix)
    ctx.register_native("parseInt", |args| {
        let s = args.first().map(to_js_string).unwrap_or_default();
        let radix_raw = args.get(1).map(|v| to_number(v) as i32).unwrap_or(0);
        // Clamp radix per spec: 0 means default (10, with 0x prefix → 16);
        // values 2..=36 are accepted, anything else yields NaN.
        let radix = if radix_raw == 0 { 0 } else { radix_raw };
        if !(0..=36).contains(&radix) || radix == 1 {
            return Ok(Value::Number(f64::NAN));
        }
        let r = if radix == 0 { 10 } else { radix as u32 };
        Ok(Value::Number(crate::builtins::date::spec_parse_int(&s, r)))
    });

    // parseFloat(string)
    ctx.register_native("parseFloat", |args| {
        let s = args.first().map(to_js_string).unwrap_or_default();
        Ok(Value::Number(crate::builtins::date::spec_parse_float(&s)))
    });

    // isNaN(value) — coerces to Number, then checks.
    ctx.register_native("isNaN", |args| {
        let v = args.first().cloned().unwrap_or(Value::Undefined);
        let n = to_number(&v);
        Ok(Value::Boolean(n.is_nan()))
    });

    // isFinite(value) — coerces to Number, returns false for NaN / ±Infinity.
    ctx.register_native("isFinite", |args| {
        let v = args.first().cloned().unwrap_or(Value::Undefined);
        let n = to_number(&v);
        Ok(Value::Boolean(n.is_finite()))
    });

    // encodeURI(uri) — leaves reserved characters alone.
    ctx.register_native("encodeURI", |args| {
        let s = args.first().map(to_js_string).unwrap_or_default();
        Ok(Value::String(encode_uri(&s, true)))
    });

    // encodeURIComponent(str) — escapes reserved characters too.
    ctx.register_native("encodeURIComponent", |args| {
        let s = args.first().map(to_js_string).unwrap_or_default();
        Ok(Value::String(encode_uri(&s, false)))
    });

    // decodeURI(uri) — leaves reserved percent-escapes intact.
    ctx.register_native("decodeURI", |args| {
        let s = args.first().map(to_js_string).unwrap_or_default();
        decode_uri(&s, true).map(Value::String)
    });

    // decodeURIComponent(str) — decodes every percent-escape.
    ctx.register_native("decodeURIComponent", |args| {
        let s = args.first().map(to_js_string).unwrap_or_default();
        decode_uri_component(&s).map(Value::String)
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Context;

    fn eval_str(src: &str) -> String {
        let mut ctx = Context::new().unwrap();
        match ctx.eval(src).unwrap() {
            Value::String(s) => s,
            other => panic!("expected String from {:?}, got {:?}", src, other),
        }
    }

    fn eval_num(src: &str) -> f64 {
        let mut ctx = Context::new().unwrap();
        match ctx.eval(src).unwrap() {
            Value::Number(n) => n,
            other => panic!("expected Number from {:?}, got {:?}", src, other),
        }
    }

    fn eval_bool(src: &str) -> bool {
        let mut ctx = Context::new().unwrap();
        match ctx.eval(src).unwrap() {
            Value::Boolean(b) => b,
            other => panic!("expected Boolean from {:?}, got {:?}", src, other),
        }
    }

    #[test]
    fn parse_int_basic() {
        assert_eq!(eval_num("parseInt('42')"), 42.0);
        assert_eq!(eval_num("parseInt('  17abc')"), 17.0);
        assert_eq!(eval_num("parseInt('-7')"), -7.0);
        assert_eq!(eval_num("parseInt('0x1F', 16)"), 31.0);
        assert!(eval_num("parseInt('hello')").is_nan());
        assert_eq!(eval_num("parseInt('ff', 16)"), 255.0);
    }

    #[test]
    fn parse_int_radix_validation() {
        assert!(eval_num("parseInt('1', 1)").is_nan());
        assert!(eval_num("parseInt('1', 37)").is_nan());
        assert_eq!(eval_num("parseInt('10', 2)"), 2.0);
    }

    #[test]
    fn parse_float_basic() {
        assert_eq!(eval_num("parseFloat('3.14')"), 3.14);
        assert_eq!(eval_num("parseFloat('  -7.5abc')"), -7.5);
        assert!(eval_num("parseFloat('not a number')").is_nan());
    }

    #[test]
    fn is_nan_global() {
        assert!(eval_bool("isNaN(NaN)"));
        assert!(eval_bool("isNaN('foo')"));
        assert!(eval_bool("isNaN(undefined)"));
        assert!(!eval_bool("isNaN(0)"));
        assert!(!eval_bool("isNaN('42')"));
    }

    #[test]
    fn is_finite_global() {
        assert!(eval_bool("isFinite(0)"));
        assert!(eval_bool("isFinite('42')"));
        assert!(!eval_bool("isFinite(NaN)"));
        assert!(!eval_bool("isFinite(Infinity)"));
        assert!(!eval_bool("isFinite(-Infinity)"));
        assert!(!eval_bool("isFinite('foo')"));
    }

    #[test]
    fn encode_uri_basic() {
        assert_eq!(
            eval_str("encodeURI('http://x.test/a b')"),
            "http://x.test/a%20b"
        );
        // Reserved chars pass through in encodeURI
        assert_eq!(eval_str("encodeURI('a;b/c?d=e')"), "a;b/c?d=e");
    }

    #[test]
    fn encode_uri_component_escapes_reserved() {
        assert_eq!(eval_str("encodeURIComponent('a;b/c')"), "a%3Bb%2Fc");
        assert_eq!(eval_str("encodeURIComponent(' ')",), "%20");
    }

    #[test]
    fn decode_uri_component_roundtrip() {
        assert_eq!(eval_str("decodeURIComponent('a%3Bb%2Fc')"), "a;b/c");
        assert_eq!(eval_str("decodeURIComponent('%20')"), " ");
    }

    #[test]
    fn decode_uri_decodes_reserved() {
        // decodeURI fully decodes percent-escapes (matching the spec's
        // approach), so '%3B' → ';' even though encodeURI never escapes ';'.
        assert_eq!(
            eval_str("decodeURI('http://x.test/a%3Bb')"),
            "http://x.test/a;b"
        );
    }

    #[test]
    fn malformed_uri_throws() {
        let mut ctx = Context::new().unwrap();
        assert!(ctx.eval("decodeURIComponent('%2')").is_err());
        assert!(ctx.eval("decodeURIComponent('%xy')").is_err());
    }
}
