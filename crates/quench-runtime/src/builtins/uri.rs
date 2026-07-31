//! URI handling functions: encodeURI, encodeURIComponent, decodeURI,
//! decodeURIComponent, plus the global parseInt / parseFloat / isNaN /
//! isFinite. parseInt / parseFloat are exposed both as globals and as
//! properties of Number; the actual logic lives in
//! `builtins::date::spec_parse_int` / `spec_parse_float` (parseInt/parseFloat
//! are simpler than Date parsing, but the implementation already covers
//! the spec cases).

use std::rc::Rc;

use crate::value::{to_js_string, to_number, try_to_number, Value};
use crate::Context;

/// RFC 3986 "unreserved" characters plus a few reserved characters that
/// decodeURI / decodeURIComponent treat as legal.
fn is_uri_unreserved(c: char) -> bool {
    c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.' | '~' | '!' | '*' | '\'' | '(' | ')')
}

/// Characters reserved by RFC 3986 that encodeURI leaves alone (the
/// "reserved" set minus characters that encodeURIComponent also escapes).
fn is_uri_reserved(c: char) -> bool {
    matches!(c, ';' | ',' | '/' | ':' | '&' | '=' | '+' | '$' | '?' | '@')
}

/// Decode a single percent-escape `%XX` to a byte (0..=255). Returns None
/// when the escape is malformed.
fn decode_escape(bytes: &[u8]) -> Option<u8> {
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

fn lone_surrogate_at(bytes: &[u8], i: usize) -> Option<u16> {
    if i + 7 > bytes.len() || bytes[i..].get(..3)? != [0xef, 0xbf, 0xbd] {
        return None;
    }
    let hex = std::str::from_utf8(&bytes[i + 3..i + 7]).ok()?;
    let value = u16::from_str_radix(hex, 16).ok()?;
    (0xd800..=0xdfff).contains(&value).then_some(value)
}

fn surrogate_width(bytes: &[u8], i: usize) -> Option<(u16, usize)> {
    lone_surrogate_at(bytes, i).map(|value| (value, 7))
}

fn encode_uri(s: &str, keep_reserved: bool) -> Result<String, crate::JsError> {
    let mut out = String::with_capacity(s.len());
    let bytes = s.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        let b = bytes[i];
        if let Some((high, width)) = surrogate_width(bytes, i) {
            if (0xd800..=0xdbff).contains(&high) {
                if let Some((low, low_width)) = surrogate_width(bytes, i + width) {
                    if (0xdc00..=0xdfff).contains(&low) {
                        let code = 0x10000 + ((high as u32 - 0xd800) << 10) + low as u32 - 0xdc00;
                        let mut encoded = [0u8; 4];
                        let character =
                            std::char::from_u32(code).ok_or_else(|| uri_error("URI malformed"))?;
                        for byte in character.encode_utf8(&mut encoded).as_bytes() {
                            out.push_str(&format!("%{byte:02X}"));
                        }
                        i += width + low_width;
                        continue;
                    }
                }
            }
            return Err(uri_error("URI malformed"));
        }
        // Pass through ASCII printable characters that don't need encoding.
        if b < 0x80 {
            let c = b as char;
            if is_uri_unreserved(c) || (keep_reserved && is_uri_reserved(c)) {
                out.push(c);
            } else if keep_reserved && matches!(b, b'#') {
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
    Ok(out)
}

/// Throw a URIError and return a JsError.
fn uri_error(msg: impl Into<String>) -> crate::JsError {
    let (err, js_err) = crate::value::error::create_js_error_with_type(&msg.into(), "URIError");
    crate::value::set_thrown_value(err);
    js_err
}

fn decode_uri(s: &str, keep_reserved: bool) -> Result<String, crate::JsError> {
    let bytes = s.as_bytes();
    let mut out = String::with_capacity(s.len());
    let mut i = 0;
    while i < bytes.len() {
        let b = bytes[i];
        if b == b'%' {
            if i + 2 >= bytes.len() {
                return Err(uri_error("URI malformed"));
            }
            let start = i;
            let first =
                decode_escape(&bytes[i..i + 3]).ok_or_else(|| uri_error("URI malformed"))?;
            let width = match first {
                0..=0x7f => 1,
                0xc2..=0xdf => 2,
                0xe0..=0xef => 3,
                0xf0..=0xf4 => 4,
                _ => return Err(uri_error("URI malformed")),
            };
            let mut encoded = [0u8; 4];
            encoded[0] = first;
            i += 3;
            for byte in encoded.iter_mut().take(width).skip(1) {
                if i + 2 >= bytes.len() || bytes[i] != b'%' {
                    return Err(uri_error("URI malformed"));
                }
                *byte =
                    decode_escape(&bytes[i..i + 3]).ok_or_else(|| uri_error("URI malformed"))?;
                i += 3;
            }
            let decoded =
                std::str::from_utf8(&encoded[..width]).map_err(|_| uri_error("URI malformed"))?;
            if keep_reserved && width == 1 && is_uri_reserved(first as char) {
                out.push_str(&s[start..i]);
            } else {
                out.push_str(decoded);
            }
        } else if b < 0x80 {
            out.push(b as char);
            i += 1;
        } else {
            let c = s[i..]
                .chars()
                .next()
                .ok_or_else(|| uri_error("URI malformed"))?;
            out.push(c);
            i += c.len_utf8();
        }
    }
    Ok(out)
}

fn uri_argument(value: Option<&Value>) -> Result<String, crate::JsError> {
    let value = value.unwrap_or(&Value::Undefined);
    let primitive = crate::value::to_primitive(value, Some("string"))?;
    Ok(to_js_string(&primitive))
}

/// Re-escape characters that encode_uri (with keep_reserved) would have
/// escaped. Mirrors encode_uri so the two functions form a round-trip.
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
            let v = decode_escape(&bytes[i..i + 3]).ok_or_else(|| uri_error("URI malformed"))?;
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

/// Convert the first argument to a primitive string per ECMA-262 ToString,
/// including ToPrimitive with hint "string" for objects. Unlike `to_js_string`,
/// this propagates thrown errors from `toString` / `valueOf` so callers like
/// parseFloat / parseInt can surface them (test262 parseFloat T7 #7).
fn to_string_for_spec(arg: &Value) -> Result<String, crate::JsError> {
    if let Some(s) = crate::value::convert::simple_string_value(arg) {
        return Ok(s);
    }
    if matches!(arg, Value::Symbol(_)) {
        let (err, js_err) = crate::value::error::create_js_error_with_type(
            "Cannot convert a Symbol to a string",
            "TypeError",
        );
        crate::value::set_thrown_value(err);
        return Err(js_err);
    }
    if let Value::Object(o) = arg {
        // Try toString first, then valueOf. A throw from either propagates;
        // a non-primitive result triggers the fall-through to the next method.
        let try_call = |name: &str| -> Result<Option<String>, crate::JsError> {
            let method = o.borrow().get(name);
            if !matches!(
                method,
                Some(Value::Function(_) | Value::NativeFunction(_) | Value::NativeConstructor(_))
            ) {
                return Ok(None);
            }
            let m = method.unwrap();
            let res = crate::eval::call_value_with_this(m, vec![], Value::Object(Rc::clone(o)))?;
            Ok(crate::value::convert::simple_string_value(&res))
        };
        if let Some(s) = try_call("toString")? {
            return Ok(s);
        }
        if let Some(s) = try_call("valueOf")? {
            return Ok(s);
        }
        // Neither yielded a primitive → TypeError per spec.
        let (err, js_err) = crate::value::error::create_js_error_with_type(
            "Cannot convert object to primitive value",
            "TypeError",
        );
        crate::value::set_thrown_value(err);
        return Err(js_err);
    }
    Ok(crate::value::to_js_string(arg))
}

pub fn register_uri(ctx: &mut Context) {
    // parseInt(string, radix)
    ctx.register_native("parseInt", |args| {
        let arg = args.first().cloned().unwrap_or(Value::Undefined);
        let s = to_string_for_spec(&arg)?;
        let radix_raw = args.get(1).map(to_number).unwrap_or(0.0);
        // ToInt32 per spec: NaN/Infinity → 0, wrapping at 2^32.
        let radix = if radix_raw.is_nan() || radix_raw.is_infinite() {
            0i32
        } else {
            let wrapped = radix_raw.trunc().rem_euclid(4_294_967_296.0);
            if wrapped >= 2_147_483_648.0 {
                (wrapped - 4_294_967_296.0) as i32
            } else {
                wrapped as i32
            }
        };
        // Clamp radix per spec: 0 means default (10, with 0x prefix → 16);
        // values 2..=36 are accepted, anything else yields NaN.
        if radix != 0 && !(2..=36).contains(&radix) {
            return Ok(Value::Number(f64::NAN));
        }
        let r = if radix == 0 { 10 } else { radix as u32 };
        Ok(Value::Number(crate::builtins::date::spec_parse_int(&s, r)))
    });

    // parseFloat(string)
    ctx.register_native("parseFloat", |args| {
        let arg = args.first().cloned().unwrap_or(Value::Undefined);
        let s = to_string_for_spec(&arg)?;
        Ok(Value::Number(crate::builtins::date::spec_parse_float(&s)))
    });

    // isNaN(value) — coerces to Number, then checks.
    ctx.register_native("isNaN", |args| {
        let v = args.first().cloned().unwrap_or(Value::Undefined);
        let n = try_to_number(&v)?;
        Ok(Value::Boolean(n.is_nan()))
    });

    // isFinite(value) — coerces to Number, returns false for NaN / ±Infinity.
    ctx.register_native("isFinite", |args| {
        let v = args.first().cloned().unwrap_or(Value::Undefined);
        let n = try_to_number(&v)?;
        Ok(Value::Boolean(n.is_finite()))
    });

    // encodeURI(uri) — leaves reserved characters alone.
    ctx.register_native("encodeURI", |args| {
        let s = uri_argument(args.first())?;
        Ok(Value::String(encode_uri(&s, true)?))
    });

    // encodeURIComponent(str) — escapes reserved characters too.
    ctx.register_native("encodeURIComponent", |args| {
        let s = uri_argument(args.first())?;
        Ok(Value::String(encode_uri(&s, false)?))
    });

    // decodeURI(uri) — leaves reserved percent-escapes intact.
    ctx.register_native("decodeURI", |args| {
        let s = uri_argument(args.first())?;
        decode_uri(&s, true).map(Value::String)
    });

    // decodeURIComponent(str) — decodes every percent-escape.
    ctx.register_native("decodeURIComponent", |args| {
        let s = uri_argument(args.first())?;
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
    fn parse_int_radix_uses_to_int32_wrapping() {
        assert_eq!(eval_num("parseInt('11', 4294967298)"), 3.0);
    }

    #[test]
    fn parse_int_has_standard_function_name_descriptor() {
        let mut ctx = Context::new().unwrap();
        assert_eq!(
            ctx.eval("var d = Object.getOwnPropertyDescriptor(parseInt, 'name'); [d.value, d.writable, d.enumerable, d.configurable].join('|')").unwrap(),
            Value::String("parseInt|false|false|true".into())
        );
    }

    #[test]
    fn parse_int_has_configurable_length_descriptor() {
        let mut ctx = Context::new().unwrap();
        assert_eq!(
            ctx.eval("var d = Object.getOwnPropertyDescriptor(parseInt, 'length'); var deleted = delete parseInt.length; [d.value, d.writable, d.enumerable, d.configurable, deleted, Object.prototype.hasOwnProperty.call(parseInt, 'length')].join('|')").unwrap(),
            Value::String("2|false|false|true|true|false".into())
        );
    }

    #[test]
    fn parse_float_exponent_matches_decimal_literal() {
        assert_eq!(crate::builtins::date::spec_parse_float("0.1e-1x"), 0.01);
    }

    #[test]
    fn parse_int_name_can_be_deleted_when_configurable() {
        let mut ctx = Context::new().unwrap();
        assert_eq!(
            ctx.eval("var old = parseInt.name; parseInt.name = 'unlikelyValue'; var write = parseInt.name === 'unlikelyValue'; var had = Object.prototype.hasOwnProperty.call(parseInt, 'name'); var deleted = delete parseInt.name; var after = Object.prototype.hasOwnProperty.call(parseInt, 'name'); [old, write, had, deleted, after].join('|')").unwrap(),
            Value::String("parseInt|false|true|true|false".into())
        );
    }

    #[test]
    fn parse_int_length_assignment_is_ignored() {
        let mut ctx = Context::new().unwrap();
        assert_eq!(
            ctx.eval("parseInt.length = 'unlikelyValue'; parseInt.length")
                .unwrap(),
            Value::Number(2.0)
        );
    }

    #[test]
    fn unary_global_function_lengths_match_spec() {
        let mut ctx = Context::new().unwrap();
        assert_eq!(
            ctx.eval("[isNaN.length, isFinite.length].join('|')")
                .unwrap(),
            Value::String("1|1".into())
        );
    }

    #[test]
    fn parse_float_basic() {
        let expected = eval_num("3.14");
        assert!((eval_num("parseFloat('3.14')") - expected).abs() < 1e-10);
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
    fn is_finite_name_property() {
        // isFinite.name should be "isFinite"
        assert_eq!(eval_str("isFinite.name"), "isFinite");
        // isFinite.name should be non-writable (native function names are non-writable)
        assert!(eval_bool(
            "!Object.getOwnPropertyDescriptor(isFinite, 'name').writable"
        ));
        // Note: property descriptor for native function name is not fully
        // implemented (getOwnPropertyDescriptor uses Object path, not NativeFunction path).
        // The direct .name access is verified above.
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
    fn encode_uri_preserves_at_sign() {
        assert_eq!(eval_str("encodeURI('@')"), "@");
    }

    #[test]
    fn encode_uri_component_escapes_hash() {
        assert_eq!(eval_str("encodeURIComponent('#')"), "%23");
    }

    #[test]
    fn eval_length_can_be_deleted() {
        let mut ctx = Context::new().unwrap();
        assert_eq!(
            ctx.eval("delete eval.length; eval.hasOwnProperty('length')")
                .unwrap(),
            Value::Boolean(false)
        );
    }

    #[test]
    fn eval_name_is_configurable() {
        let mut ctx = Context::new().unwrap();
        assert_eq!(
            ctx.eval("Object.getOwnPropertyDescriptor(eval, 'name').configurable")
                .unwrap(),
            Value::Boolean(true)
        );
    }

    #[test]
    fn strict_arguments_share_throw_type_error_per_realm() {
        let mut ctx = Context::new().unwrap();
        assert_eq!(
            ctx.eval("var a = (function() { 'use strict'; return arguments; })(); var b = (function() { 'use strict'; return arguments; })(); Object.getOwnPropertyDescriptor(a, 'callee').get === Object.getOwnPropertyDescriptor(b, 'callee').get").unwrap(),
            Value::Boolean(true)
        );
    }

    #[test]
    fn uri_function_lengths_are_unary() {
        let mut ctx = Context::new().unwrap();
        assert_eq!(
            ctx.eval("[encodeURI.length, encodeURIComponent.length, decodeURI.length, decodeURIComponent.length].join('|')").unwrap(),
            Value::String("1|1|1|1".into())
        );
    }

    #[test]
    fn encode_uri_uses_string_hint_primitive_conversion() {
        let mut ctx = Context::new().unwrap();
        assert_eq!(
            ctx.eval("encodeURI({valueOf: function() { return '^'; }, toString: function() { return {}; }})").unwrap(),
            Value::String("%5E".into())
        );
    }

    #[test]
    fn decode_uri_component_roundtrip() {
        assert_eq!(eval_str("decodeURIComponent('a%3Bb%2Fc')"), "a;b/c");
        assert_eq!(eval_str("decodeURIComponent('%20')"), " ");
    }

    #[test]
    fn decode_uri_rejects_malformed_escape_with_unicode_tail() {
        let mut ctx = Context::new().unwrap();
        assert_eq!(
            ctx.eval("try { decodeURI('%C0%' + String.fromCharCode(0x800, 0x800)); false } catch (e) { e instanceof URIError }").unwrap(),
            Value::Boolean(true)
        );
    }

    #[test]
    fn decode_uri_preserves_unescaped_nul() {
        let mut ctx = Context::new().unwrap();
        assert_eq!(
            ctx.eval("decodeURI(String.fromCharCode(0)) === String.fromCharCode(0)")
                .unwrap(),
            Value::Boolean(true)
        );
    }

    #[test]
    fn decode_uri_rejects_invalid_percent_utf8() {
        let mut ctx = Context::new().unwrap();
        assert_eq!(
            ctx.eval("try { decodeURI('%C0%00'); false } catch (e) { e instanceof URIError }")
                .unwrap(),
            Value::Boolean(true)
        );
    }

    #[test]
    fn decode_uri_rejects_trailing_percent() {
        let mut ctx = Context::new().unwrap();
        assert_eq!(
            ctx.eval("try { decodeURI('%'); false } catch (e) { e instanceof URIError }")
                .unwrap(),
            Value::Boolean(true)
        );
    }

    #[test]
    fn encode_uri_rejects_lone_low_surrogate() {
        let mut ctx = Context::new().unwrap();
        assert_eq!(
            ctx.eval("try { encodeURI(String.fromCharCode(0xDC00)); false } catch (e) { e instanceof URIError }").unwrap(),
            Value::Boolean(true)
        );
    }

    #[test]
    fn encode_uri_encodes_surrogate_pair() {
        let mut ctx = Context::new().unwrap();
        assert_eq!(
            ctx.eval("encodeURI(String.fromCharCode(0xD83D, 0xDE00))")
                .unwrap(),
            Value::String("%F0%9F%98%80".into())
        );
    }

    #[test]
    fn decode_uri_decodes_reserved() {
        assert_eq!(
            eval_str("decodeURI('http://x.test/a%3Bb')"),
            "http://x.test/a%3Bb"
        );
    }

    #[test]
    fn malformed_uri_throws() {
        let mut ctx = Context::new().unwrap();
        assert!(ctx.eval("decodeURIComponent('%2')").is_err());
        assert!(ctx.eval("decodeURIComponent('%xy')").is_err());
    }
}
