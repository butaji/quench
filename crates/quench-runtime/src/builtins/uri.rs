//! URI handling functions: encodeURI, encodeURIComponent, decodeURI,
//! decodeURIComponent, plus the global parseInt / parseFloat / isNaN /
//! isFinite. parseInt / parseFloat are exposed both as globals and as
//! properties of Number; the actual logic lives in
//! `builtins::date::spec_parse_int` / `spec_parse_float` (parseInt/parseFloat
//! are simpler than Date parsing, but the implementation already covers
//! the spec cases).

use std::rc::Rc;

use percent_encoding::{percent_decode, utf8_percent_encode, AsciiSet, NON_ALPHANUMERIC};

use crate::value::wtf8::append_wtf8_surrogate;
use crate::value::{to_js_string, to_number, try_to_number, Value};
use crate::Context;

fn is_uri_reserved_byte(c: u8) -> bool {
    matches!(
        c,
        b';' | b',' | b'/' | b':' | b'&' | b'=' | b'+' | b'$' | b'?' | b'@' | b'#'
    )
}

fn hex_digit(b: u8) -> Option<u8> {
    match b {
        b'0'..=b'9' => Some(b - b'0'),
        b'a'..=b'f' => Some(b - b'a' + 10),
        b'A'..=b'F' => Some(b - b'A' + 10),
        _ => None,
    }
}

fn is_oxc_utf16_surrogate_text(s: &str) -> bool {
    let bytes = s.as_bytes();
    bytes.len() == 7
        && bytes.starts_with(&[0xEF, 0xBF, 0xBD])
        && bytes[3].is_ascii_hexdigit()
        && bytes[4].is_ascii_hexdigit()
        && bytes[5].is_ascii_hexdigit()
        && bytes[6].is_ascii_hexdigit()
        && u32::from_str_radix(std::str::from_utf8(&bytes[3..7]).unwrap_or(""), 16)
            .is_ok_and(|code_point| (0xD800..=0xDFFF).contains(&code_point))
}

fn normalize_for_uri_encoding(s: &str) -> Result<String, crate::JsError> {
    let mut out = String::with_capacity(s.len());
    for chunk in crate::value::wtf8::wtf8_for_of_iterate(s) {
        if let Value::String(chunk) = chunk {
            if is_oxc_utf16_surrogate_text(&chunk) {
                return Err(uri_error("URI malformed"));
            }
            out.push_str(&chunk);
            continue;
        }
        return Err(uri_error("URI malformed"));
    }
    Ok(out)
}

fn canonicalize_hex_case(s: &str) -> String {
    let bytes = s.as_bytes();
    let mut out = String::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%'
            && i + 2 < bytes.len()
            && bytes[i + 1].is_ascii_hexdigit()
            && bytes[i + 2].is_ascii_hexdigit()
        {
            out.push('%');
            out.push((bytes[i + 1] as char).to_ascii_uppercase());
            out.push((bytes[i + 2] as char).to_ascii_uppercase());
            i += 3;
            continue;
        }
        out.push(bytes[i] as char);
        i += 1;
    }
    out
}

const ENCODE_URI_COMPONENT_SET: &AsciiSet = &NON_ALPHANUMERIC
    .remove(b'-')
    .remove(b'_')
    .remove(b'.')
    .remove(b'~')
    .remove(b'!')
    .remove(b'\'')
    .remove(b'(')
    .remove(b')')
    .remove(b'*');

const ENCODE_URI_SET: &AsciiSet = &ENCODE_URI_COMPONENT_SET
    .remove(b';')
    .remove(b',')
    .remove(b'/')
    .remove(b':')
    .remove(b'@')
    .remove(b'&')
    .remove(b'=')
    .remove(b'+')
    .remove(b'$')
    .remove(b'?')
    .remove(b'#');

fn append_wtf8_string(out: &mut String, decoded: &str) {
    for ch in decoded.chars() {
        let cp = ch as u32;
        if cp <= 0xFFFF {
            out.push(ch);
            continue;
        }
        let value = cp - 0x10000;
        let high = 0xD800 + ((value >> 10) & 0x3FF);
        let low = 0xDC00 + (value & 0x3FF);
        append_wtf8_surrogate(out, high as u16);
        append_wtf8_surrogate(out, low as u16);
    }
}

fn strict_percent_byte(bytes: &[u8], i: usize) -> Result<u8, crate::JsError> {
    if i + 2 >= bytes.len() {
        return Err(uri_error("URI malformed"));
    }
    let h1 = hex_digit(bytes[i + 1]).ok_or_else(|| uri_error("URI malformed"))?;
    let h2 = hex_digit(bytes[i + 2]).ok_or_else(|| uri_error("URI malformed"))?;
    Ok((h1 << 4) | h2)
}

fn decode_percent_triplet_sequence(
    s: &str,
    keep_reserved: bool,
) -> Option<Result<String, crate::JsError>> {
    let bytes = s.as_bytes();
    if !bytes.len().is_multiple_of(3) || bytes.is_empty() || bytes.len() > 12 {
        return None;
    }

    let count = bytes.len() / 3;
    if count > 4 {
        return None;
    }

    let mut decoded = [0u8; 4];
    let mut index = 0usize;
    while index < bytes.len() {
        if bytes[index] != b'%' {
            return None;
        }
        let value = match strict_percent_byte(bytes, index) {
            Ok(value) => value,
            Err(err) => return Some(Err(err)),
        };
        if index / 3 >= decoded.len() {
            return None;
        }
        decoded[index / 3] = value;
        index += 3;
    }

    let b0 = decoded[0];
    if keep_reserved && decoded[..count].iter().copied().any(is_uri_reserved_byte) {
        return None;
    }
    let mut result = String::new();
    let cp = match count {
        1 => {
            if b0 >= 0x80 {
                return Some(Err(uri_error("URI malformed")));
            }
            if keep_reserved && is_uri_reserved_byte(b0) {
                return None;
            }
            result.push(b0 as char);
            return Some(Ok(result));
        }
        2 => {
            let b1 = decoded[1];
            if b0 < 0xC2 || b0 > 0xDF || (b1 & 0xC0) != 0x80 {
                return Some(Err(uri_error("URI malformed")));
            }
            ((b0 as u32 & 0x1F) << 6) | (b1 as u32 & 0x3F)
        }
        3 => {
            let b1 = decoded[1];
            let b2 = decoded[2];
            if b0 < 0xE0 || b0 > 0xEF || (b1 & 0xC0) != 0x80 || (b2 & 0xC0) != 0x80 {
                return Some(Err(uri_error("URI malformed")));
            }
            let code = ((b0 as u32 & 0x0F) << 12) | ((b1 as u32 & 0x3F) << 6) | (b2 as u32 & 0x3F);
            if code < 0x0800 || (0xD800..=0xDFFF).contains(&code) || (b0 == 0xE0 && b1 < 0xA0) {
                return Some(Err(uri_error("URI malformed")));
            }
            code
        }
        4 => {
            let b1 = decoded[1];
            let b2 = decoded[2];
            let b3 = decoded[3];
            if b0 < 0xF0
                || b0 > 0xF4
                || (b1 & 0xC0) != 0x80
                || (b2 & 0xC0) != 0x80
                || (b3 & 0xC0) != 0x80
            {
                return Some(Err(uri_error("URI malformed")));
            }
            let code = ((b0 as u32 & 0x07) << 18)
                | ((b1 as u32 & 0x3F) << 12)
                | ((b2 as u32 & 0x3F) << 6)
                | (b3 as u32 & 0x3F);
            if code < 0x10000
                || code > 0x10FFFF
                || (b0 == 0xF0 && b1 < 0x90)
                || (b0 == 0xF4 && b1 > 0x8F)
            {
                return Some(Err(uri_error("URI malformed")));
            }
            code
        }
        _ => return None,
    };

    if let Some(ch) = std::char::from_u32(cp) {
        if cp <= 0xFFFF {
            result.push(ch);
            return Some(Ok(result));
        }
    }
    if cp > 0x10FFFF {
        return Some(Err(uri_error("URI malformed")));
    }
    let value = cp - 0x10000;
    let high = 0xD800 + ((value >> 10) & 0x3FF);
    let low = 0xDC00 + (value & 0x3FF);
    append_wtf8_surrogate(&mut result, high as u16);
    append_wtf8_surrogate(&mut result, low as u16);
    Some(Ok(result))
}

fn ensure_valid_percent_encoding(s: &str) -> Result<(), crate::JsError> {
    let bytes = s.as_bytes();
    let mut i = 0usize;
    while i < bytes.len() {
        if bytes[i] == b'%' {
            strict_percent_byte(bytes, i)?;
            i += 3;
            continue;
        }
        i += 1;
    }
    Ok(())
}

fn encode_uri(s: &str, keep_reserved: bool) -> Result<String, crate::JsError> {
    let normalized = normalize_for_uri_encoding(s)?;
    let encoded = if keep_reserved {
        utf8_percent_encode(&normalized, ENCODE_URI_SET).to_string()
    } else {
        utf8_percent_encode(&normalized, ENCODE_URI_COMPONENT_SET).to_string()
    };
    Ok(canonicalize_hex_case(&encoded))
}

/// Throw a URIError and return a JsError.
fn uri_error(msg: impl Into<String>) -> crate::JsError {
    let (err, js_err) = crate::value::error::create_js_error_with_type(&msg.into(), "URIError");
    crate::value::set_thrown_value(err);
    js_err
}

fn decode_uri(s: &str, keep_reserved: bool) -> Result<String, crate::JsError> {
    if !s.contains('%') {
        return Ok(s.to_string());
    }

    if let Some(decoded) = decode_percent_triplet_sequence(s, keep_reserved) {
        return decoded;
    }

    ensure_valid_percent_encoding(s)?;

    let bytes = s.as_bytes();
    if !keep_reserved {
        let decoded = percent_decode(bytes)
            .decode_utf8()
            .map_err(|_| uri_error("URI malformed"))?;
        let mut result = String::with_capacity(decoded.len());
        append_wtf8_string(&mut result, &decoded);
        return Ok(result);
    }

    let mut has_reserved = false;
    let mut i = 0usize;
    while i < bytes.len() {
        if bytes[i] == b'%' {
            let decoded = strict_percent_byte(bytes, i)?;
            if is_uri_reserved_byte(decoded) {
                has_reserved = true;
                break;
            }
            i += 3;
            continue;
        }
        i += 1;
    }

    if !has_reserved {
        return decode_uri_component_raw(s);
    }

    let mut out = String::with_capacity(s.len());
    let mut segment_start = 0usize;

    let mut i = 0usize;
    while i < bytes.len() {
        let byte = bytes[i];
        if byte == b'%' {
            let decoded = strict_percent_byte(bytes, i)?;
            if keep_reserved && is_uri_reserved_byte(decoded) {
                if i > segment_start {
                    let decoded = percent_decode(&bytes[segment_start..i])
                        .decode_utf8()
                        .map_err(|_| uri_error("URI malformed"))?;
                    append_wtf8_string(&mut out, &decoded);
                }
                out.push('%');
                out.push(bytes[i + 1] as char);
                out.push(bytes[i + 2] as char);
                segment_start = i + 3;
            }
            i += 3;
            continue;
        }

        i += 1;
    }

    let decoded = percent_decode(&bytes[segment_start..])
        .decode_utf8()
        .map_err(|_| uri_error("URI malformed"))?;
    append_wtf8_string(&mut out, &decoded);

    Ok(out)
}

fn decode_uri_component_raw(s: &str) -> Result<String, crate::JsError> {
    if !s.contains('%') {
        return Ok(s.to_string());
    }
    if let Some(decoded) = decode_percent_triplet_sequence(s, false) {
        return decoded;
    }
    ensure_valid_percent_encoding(s)?;

    let decoded = percent_decode(s.as_bytes())
        .decode_utf8()
        .map_err(|_| uri_error("URI malformed"))?;
    let mut result = String::with_capacity(decoded.len());
    append_wtf8_string(&mut result, &decoded);
    Ok(result)
}

fn decode_uri_component(s: &str) -> Result<String, crate::JsError> {
    decode_uri_component_raw(s)
}

fn uri_argument(value: Option<&Value>) -> Result<String, crate::JsError> {
    let value = value.unwrap_or(&Value::Undefined);
    let primitive = crate::value::to_primitive(value, Some("string"))?;
    Ok(to_js_string(&primitive))
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
    ctx.register_native("__encodeURI", |args| {
        let s = uri_argument(args.first())?;
        Ok(Value::String(encode_uri(&s, true)?))
    });

    // encodeURIComponent(str) — escapes reserved characters too.
    ctx.register_native("__encodeURIComponent", |args| {
        let s = uri_argument(args.first())?;
        Ok(Value::String(encode_uri(&s, false)?))
    });

    // decodeURI(uri) — leaves reserved percent-escapes intact.
    ctx.register_native("__decodeURI", |args| {
        let s = uri_argument(args.first())?;
        decode_uri(&s, true).map(Value::String)
    });

    // decodeURIComponent(str) — decodes every percent-escape.
    ctx.register_native("__decodeURIComponent", |args| {
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
    fn decode_uri_preserves_encoded_hash() {
        assert_eq!(eval_str("decodeURI('%23')"), "%23");
    }

    #[test]
    fn decode_uri_preserves_reserved_escape_case() {
        assert_eq!(eval_str("decodeURI('%3b')"), "%3b");
        assert_eq!(eval_str("decodeURI('%2F%3A%3f%23')"), "%2F%3A%3f%23");
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
    fn encode_uri_outputs_uppercase_hex() {
        assert_eq!(
            eval_str("encodeURI('[object Object]')"),
            "%5Bobject%20Object%5D"
        );
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
    fn encode_uri_rejects_lone_high_surrogate() {
        let mut ctx = Context::new().unwrap();
        assert_eq!(
            ctx.eval("try { encodeURI(String.fromCharCode(0xD800)); false } catch (e) { e instanceof URIError }").unwrap(),
            Value::Boolean(true)
        );
    }

    #[test]
    fn encode_uri_component_rejects_lone_high_surrogate() {
        let mut ctx = Context::new().unwrap();
        assert_eq!(
            ctx.eval("try { encodeURIComponent(String.fromCharCode(0xD800)); false } catch (e) { e instanceof URIError }").unwrap(),
            Value::Boolean(true)
        );
    }

    #[test]
    fn encode_uri_component_rejects_truncated_surrogate_pair() {
        let mut ctx = Context::new().unwrap();
        assert_eq!(
            ctx.eval("try { encodeURIComponent(String.fromCharCode(0xD800, 0x0041)); false } catch (e) { e instanceof URIError }").unwrap(),
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
        assert!(ctx.eval("decodeURIComponent('%1')").is_err());
        assert!(ctx.eval("decodeURIComponent('% ')").is_err());
        assert!(ctx.eval("decodeURIComponent('%xy')").is_err());
        assert!(ctx.eval("decodeURIComponent('%')").is_err());
        assert!(ctx.eval("decodeURIComponent('%1?')").is_err());
        assert!(ctx.eval("decodeURIComponent('%a-')").is_err());
    }
}
