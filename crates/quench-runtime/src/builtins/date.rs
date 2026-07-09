//! Date built-in and global utility functions

use std::cell::RefCell;
use std::rc::Rc;

use crate::value::{
    to_js_string, to_number, to_bool, NativeConstructor, NativeFunction, Object, ObjectKind, Value,
};
use crate::Context;

// ============================================================================
// parseInt and parseFloat (ECMAScript spec compliant)
// ============================================================================

fn is_whitespace(c: char) -> bool {
    matches!(c, ' ' | '\t' | '\n' | '\r' | '\x0b' | '\x0c' | '\u{00a0}'
        | '\u{1680}' | '\u{2000}'..='\u{200a}' | '\u{2028}' | '\u{2029}'
        | '\u{202f}' | '\u{205f}' | '\u{3000}' | '\u{feff}')
}

fn is_digit_in_radix(c: char, radix: u32) -> Option<u32> {
    let c = c.to_ascii_lowercase();
    let val = if c.is_ascii_digit() {
        c.to_digit(10)?
    } else if c.is_ascii_lowercase() {
        c.to_digit(36)?
    } else {
        return None;
    };
    if val < radix {
        Some(val)
    } else {
        None
    }
}

fn spec_parse_int(string: &str, mut radix: u32) -> f64 {
    let s = string.trim_start_matches(is_whitespace);
    if s.is_empty() {
        return f64::NAN;
    }

    let mut chars = s.chars();
    let mut sign = 1f64;

    let first = chars.next().unwrap();
    if first == '-' {
        sign = -1.0;
    } else if first == '+' {
        // positive, sign stays 1.0
    } else {
        // Not a sign, put it back conceptually by starting from first
        let remaining: String = std::iter::once(first).chain(chars).collect();
        // Detect hex prefix for auto radix
        if radix == 10 {
            if remaining.starts_with("0x") || remaining.starts_with("0X") {
                radix = 16;
            }
        }
        return parse_int_value(&remaining, radix, sign);
    }

    let remaining: String = chars.collect();
    // Detect hex prefix for auto radix
    if radix == 10 {
        if remaining.starts_with("0x") || remaining.starts_with("0X") {
            radix = 16;
        }
    }
    parse_int_value(&remaining, radix, sign)
}

fn parse_int_value(s: &str, radix: u32, sign: f64) -> f64 {
    let mut chars = s.chars().peekable();

    // Handle 0x prefix for radix 16
    let chars: Vec<char> = if radix == 16 {
        let mut c = chars;
        let prefix_chars: Vec<_> = c.by_ref().take(2).collect();
        if prefix_chars.len() == 2 && prefix_chars[0] == '0' && prefix_chars[1].to_ascii_lowercase() == 'x' {
            c.collect()
        } else {
            prefix_chars.into_iter().chain(c).collect()
        }
    } else {
        chars.collect()
    };

    let mut result: f64 = 0.0;
    let mut any_digit = false;

    for c in chars {
        if let Some(val) = is_digit_in_radix(c, radix) {
            result = result * (radix as f64) + (val as f64);
            any_digit = true;
        } else {
            break;
        }
    }

    if !any_digit {
        f64::NAN
    } else {
        result * sign
    }
}

fn spec_parse_float(string: &str) -> f64 {
    let s = string.trim_start_matches(is_whitespace);
    if s.is_empty() {
        return f64::NAN;
    }

    let mut chars = s.chars().peekable();
    let sign = parse_float_sign(&mut chars);

    // Handle hex floats
    if let Some(val) = try_parse_hex_float(&mut chars) {
        return val * sign;
    }

    // Parse decimal significand
    let (significand, has_digit) = parse_decimal_significand(&mut chars);
    if !has_digit {
        return f64::NAN;
    }

    // Handle exponent
    let significand = apply_exponent(&mut chars, significand);
    significand * sign
}

fn parse_float_sign(chars: &mut std::iter::Peekable<std::str::Chars>) -> f64 {
    if chars.peek() == Some(&'-') {
        chars.next();
        -1.0
    } else if chars.peek() == Some(&'+') {
        chars.next();
        1.0
    } else {
        1.0
    }
}

fn try_parse_hex_float(chars: &mut std::iter::Peekable<std::str::Chars>) -> Option<f64> {
    if chars.peek() != Some(&'0') {
        return None;
    }
    let mut c = chars.clone();
    c.next();
    if c.peek()?.to_ascii_lowercase() != 'x' {
        return None;
    }
    chars.next(); // consume 'x'
    let mut significand = 0.0;
    let mut has_digit = false;

    while let Some(&ch) = chars.peek() {
        if let Some(d) = ch.to_digit(16) {
            significand = significand * 16.0 + (d as f64);
            has_digit = true;
            chars.next();
        } else {
            break;
        }
    }
    if !has_digit {
        return Some(f64::NAN);
    }

    // Parse optional exponent (p for hex floats)
    if chars.peek().map(|c| c.to_ascii_lowercase()) == Some('p') {
        chars.next();
        let exp_sign = if chars.peek() == Some(&'-') { chars.next(); -1.0 }
            else if chars.peek() == Some(&'+') { chars.next(); 1.0 }
            else { 1.0 };
        let exp = parse_exponent(chars);
        significand *= 10.0_f64.powf(exp * exp_sign);
    }
    Some(significand)
}

fn parse_decimal_significand(chars: &mut std::iter::Peekable<std::str::Chars>) -> (f64, bool) {
    let mut significand = 0.0;
    let mut has_digit = false;

    while let Some(&c) = chars.peek() {
        if c.is_ascii_digit() {
            significand = significand * 10.0 + (c.to_digit(10).unwrap() as f64);
            has_digit = true;
            chars.next();
        } else {
            break;
        }
    }

    // Handle decimal point
    if chars.peek() == Some(&'.') {
        chars.next();
        let mut scale = 0.1;
        while let Some(&c) = chars.peek() {
            if c.is_ascii_digit() {
                significand += (c.to_digit(10).unwrap() as f64) * scale;
                scale *= 0.1;
                has_digit = true;
                chars.next();
            } else {
                break;
            }
        }
    }
    (significand, has_digit)
}

fn apply_exponent(chars: &mut std::iter::Peekable<std::str::Chars>, mut significand: f64) -> f64 {
    if chars.peek().map(|c| c.to_ascii_lowercase()) != Some('e') {
        return significand;
    }
    chars.next();
    let exp_sign = if chars.peek() == Some(&'-') { chars.next(); -1.0 }
        else if chars.peek() == Some(&'+') { chars.next(); 1.0 }
        else { 1.0 };
    let exp = parse_exponent(chars);
    significand * 10.0_f64.powf(exp * exp_sign)
}

fn parse_exponent(chars: &mut std::iter::Peekable<std::str::Chars>) -> f64 {
    let mut exp: f64 = 0.0;
    while let Some(&c) = chars.peek() {
        if let Some(d) = c.to_digit(10) {
            exp = exp * 10.0 + (d as f64);
            chars.next();
        } else {
            break;
        }
    }
    exp
}

// ============================================================================
// Global utility functions
// ============================================================================

pub fn register_global_functions(ctx: &mut Context) {
    register_timer_functions(ctx);
    register_parse_functions(ctx);
    register_uri_functions(ctx);
    register_type_converters(ctx);
}

fn register_timer_functions(ctx: &mut Context) {
    ctx.register_native("setTimeout", |args| {
        let _callback = args.first().map(to_js_string).unwrap_or_default();
        let _delay = args.get(1).map(|v| to_number(v) as u64).unwrap_or(0);
        Ok(Value::Number(1.0))
    });
    ctx.register_native("setInterval", |args| {
        let _callback = args.first().map(to_js_string).unwrap_or_default();
        let _interval = args.get(1).map(|v| to_number(v) as u64).unwrap_or(0);
        Ok(Value::Number(1.0))
    });
    ctx.register_native("clearTimeout", |_args| Ok(Value::Undefined));
    ctx.register_native("clearInterval", |_args| Ok(Value::Undefined));
}

fn register_parse_functions(ctx: &mut Context) {
    ctx.register_native("parseInt", |args| {
        let s = args.first().map(to_js_string).unwrap_or_default();
        let radix = args.get(1).map(|v| to_number(v) as i32).unwrap_or(0);
        let radix = if radix == 0 { 10 } else { radix };
        if radix < 2 || radix > 36 {
            return Ok(Value::Number(f64::NAN));
        }
        let n = spec_parse_int(&s, radix as u32);
        Ok(Value::Number(n))
    });
    ctx.register_native("parseFloat", |args| {
        let s = args.first().map(to_js_string).unwrap_or_default();
        let n = spec_parse_float(&s);
        Ok(Value::Number(n))
    });
    ctx.register_native("isNaN", |args| {
        let n = args.first().map(to_number).unwrap_or(f64::NAN);
        Ok(Value::Boolean(n.is_nan()))
    });
    ctx.register_native("isFinite", |args| {
        let n = args.first().map(to_number).unwrap_or(f64::NAN);
        Ok(Value::Boolean(n.is_finite()))
    });
}

fn register_uri_functions(ctx: &mut Context) {
    ctx.register_native("encodeURIComponent", |args| {
        let s = args.first().map(to_js_string).unwrap_or_default();
        Ok(Value::String(urlencoding::encode(&s).to_string()))
    });
    ctx.register_native("decodeURIComponent", |args| {
        let s = args.first().map(to_js_string).unwrap_or_default();
        let decoded = urlencoding::decode(&s).map(|d| d.to_string()).unwrap_or(s);
        Ok(Value::String(decoded))
    });
}

fn register_type_converters(ctx: &mut Context) {
    let string_fn = Value::NativeFunction(Rc::new(NativeFunction::new(|args| {
        let s = args.first().map(to_js_string).unwrap_or_default();
        Ok(Value::String(s))
    })));
    ctx.set_global("String".to_string(), string_fn);

    let boolean_fn = Value::NativeFunction(Rc::new(NativeFunction::new(|args| {
        let b = args.first().map(to_bool).unwrap_or(false);
        Ok(Value::Boolean(b))
    })));
    ctx.set_global("Boolean".to_string(), boolean_fn);
}

// ============================================================================
// Date
// ============================================================================

fn chrono_now() -> i64 {
    use std::time::{SystemTime, UNIX_EPOCH};
    SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_millis() as i64
}

pub fn register_date(ctx: &mut Context) {
    let date_proto = Object::new(ObjectKind::Date);
    let date_proto_rc = Rc::new(RefCell::new(date_proto));

    date_proto_rc.borrow_mut().set("toString", Value::NativeFunction(Rc::new(NativeFunction::new(|_args| {
        Ok(Value::String(format!("Date @ {}", chrono_now())))
    }))));
    date_proto_rc.borrow_mut().set("valueOf", Value::NativeFunction(Rc::new(NativeFunction::new(|_args| {
        Ok(Value::Number(chrono_now() as f64))
    }))));

    let date_proto_clone = Rc::clone(&date_proto_rc);
    let date_constructor = NativeConstructor::new(
        move |_args| {
            use std::time::{SystemTime, UNIX_EPOCH};
            let now = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_millis() as f64;
            let date_obj = Object::with_prototype(ObjectKind::Date, Rc::clone(&date_proto_clone));
            let date = Rc::new(RefCell::new(date_obj));
            date.borrow_mut().set("_timestamp", Value::Number(now));
            Ok(Value::Object(date))
        },
        date_proto_rc.clone(),
    );

    let date_wrapper = Object::new(ObjectKind::Ordinary);
    let date_wrapper_rc = Rc::new(RefCell::new(date_wrapper));
    date_wrapper_rc.borrow_mut().set("constructor", Value::NativeConstructor(Rc::new(date_constructor)));
    date_wrapper_rc.borrow_mut().set("prototype", Value::Object(Rc::clone(&date_proto_rc)));
    date_wrapper_rc.borrow_mut().set("now", Value::NativeFunction(Rc::new(NativeFunction::new(|_args| {
        Ok(Value::Number(chrono_now() as f64))
    }))));
    ctx.set_global("Date".to_string(), Value::Object(date_wrapper_rc));
}
