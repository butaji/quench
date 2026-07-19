//! Date built-in and global utility functions

use std::cell::RefCell;
use std::rc::Rc;

use crate::value::{
    to_bool, to_js_string, to_number, try_to_number, NativeConstructor, NativeFunction, Object,
    ObjectKind, Value,
};
use crate::Context;

// ============================================================================
// parseInt and parseFloat (ECMAScript spec compliant)
// ============================================================================

fn is_whitespace(c: char) -> bool {
    matches!(
        c,
        ' ' | '\t' | '\n' | '\r' | '\x0b' | '\x0c' | '\u{00a0}' | '\u{1680}' | '\u{2000}'
            ..='\u{200a}'
                | '\u{2028}'
                | '\u{2029}'
                | '\u{202f}'
                | '\u{205f}'
                | '\u{3000}'
                | '\u{feff}'
    )
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

pub(crate) fn spec_parse_int(string: &str, mut radix: u32) -> f64 {
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
        if radix == 10 && (remaining.starts_with("0x") || remaining.starts_with("0X")) {
            radix = 16;
        }
        return parse_int_value(&remaining, radix, sign);
    }

    let remaining: String = chars.collect();
    // Detect hex prefix for auto radix
    if radix == 10 && (remaining.starts_with("0x") || remaining.starts_with("0X")) {
        radix = 16;
    }
    parse_int_value(&remaining, radix, sign)
}

fn parse_int_value(s: &str, radix: u32, sign: f64) -> f64 {
    let chars = s.chars().peekable();

    // Handle 0x prefix for radix 16
    let chars: Vec<char> = if radix == 16 {
        let mut c = chars;
        let prefix_chars: Vec<_> = c.by_ref().take(2).collect();
        if prefix_chars.len() == 2
            && prefix_chars[0] == '0'
            && prefix_chars[1].eq_ignore_ascii_case(&'x')
        {
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

pub(crate) fn spec_parse_float(string: &str) -> f64 {
    let s = string.trim_start_matches(is_whitespace);
    if s.is_empty() {
        return f64::NAN;
    }

    let mut chars = s.chars().peekable();
    let sign = parse_float_sign(&mut chars);

    // Per ECMA-262 StrNumericLiteral, parseFloat accepts the literal "Infinity"
    // (case-sensitive — only the exact capital-I spelling) after the optional
    // sign. parseFloat("Infinity") === Infinity, parseFloat("-Infinity") ===
    // -Infinity, but parseFloat("infinity") === NaN.
    let rest: String = chars.clone().collect();
    if rest == "Infinity" {
        return f64::INFINITY * sign;
    }

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
    if !c.peek()?.eq_ignore_ascii_case(&'x') {
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
        let exp_sign = if chars.peek() == Some(&'-') {
            chars.next();
            -1.0
        } else if chars.peek() == Some(&'+') {
            chars.next();
            1.0
        } else {
            1.0
        };
        let exp = parse_exponent(chars);
        significand *= 10.0_f64.powf(exp * exp_sign);
    }
    Some(significand)
}

fn parse_decimal_significand(chars: &mut std::iter::Peekable<std::str::Chars>) -> (f64, bool) {
    let mut significand = 0.0;
    let mut has_digit = false;
    let mut frac_digits: Vec<u32> = Vec::new();

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
        while let Some(&c) = chars.peek() {
            if c.is_ascii_digit() {
                frac_digits.push(c.to_digit(10).unwrap());
                has_digit = true;
                chars.next();
            } else {
                break;
            }
        }
    }
    // Apply the fractional digits once (not per-digit) so the value
    // matches the literal — e.g. ".01" → 1 / 100 = 0.01. Doing this
    // per-digit via `scale *= 0.1` accumulates floating-point error and
    // can mis-round for inputs like ".01e+2" (gives 10 instead of 1).
    if !frac_digits.is_empty() {
        for &d in &frac_digits {
            significand = significand * 10.0 + (d as f64);
        }
        significand /= 10f64.powi(frac_digits.len() as i32);
    }
    (significand, has_digit)
}

fn apply_exponent(chars: &mut std::iter::Peekable<std::str::Chars>, significand: f64) -> f64 {
    if chars.peek().map(|c| c.to_ascii_lowercase()) != Some('e') {
        return significand;
    }
    chars.next();
    let exp_sign = if chars.peek() == Some(&'-') {
        chars.next();
        -1.0
    } else if chars.peek() == Some(&'+') {
        chars.next();
        1.0
    } else {
        1.0
    };
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
        if !(2..=36).contains(&radix) {
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
        let n = args.first().map(try_to_number).unwrap_or(Ok(f64::NAN))?;
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
    register_string_converter(ctx);
    register_boolean_converter(ctx);
}

fn register_string_converter(ctx: &mut Context) {
    let string_proto = create_string_prototype();
    let string_proto_clone = Rc::clone(&string_proto);
    let string_fn = create_string_constructor_fn(string_proto_clone);

    let string_obj = create_string_constructor_object(string_proto.clone(), string_fn.clone());
    // Set String.prototype.constructor so primitive string access returns
    // the String constructor (matches Object.prototype.constructor pattern).
    string_proto
        .borrow_mut()
        .set("constructor", Value::Object(Rc::clone(&string_obj)));
    ctx.set_global("String".to_string(), Value::Object(string_obj));
}

fn create_string_prototype() -> Rc<RefCell<Object>> {
    let string_proto = Object::new(ObjectKind::Ordinary);
    let string_proto_rc = Rc::new(RefCell::new(string_proto));
    if let Some(object_proto) = crate::builtins::get_object_prototype() {
        string_proto_rc.borrow_mut().prototype = Some(object_proto);
    }
    crate::builtins::string::methods::install_string_methods(&string_proto_rc);
    string_proto_rc
}

fn create_string_constructor_fn(string_proto_clone: Rc<RefCell<Object>>) -> Value {
    Value::NativeFunction(Rc::new(NativeFunction::new_with_prototype(
        move |args| {
            let s = args.first().map(to_js_string).unwrap_or_default();
            let this_val = crate::builtins::get_native_this().unwrap_or(Value::Undefined);
            if let Value::Object(this_obj) = this_val {
                this_obj.borrow_mut().set("0", Value::String(s.clone()));
                this_obj
                    .borrow_mut()
                    .set("length", Value::Number(s.len() as f64));
                crate::builtins::object::set_boxed_value(
                    &mut this_obj.borrow_mut(),
                    Value::String(s.clone()),
                );
                this_obj.borrow_mut().exotic_kind = Some(crate::value::kind::ExoticKind::String);
                if this_obj.borrow().prototype.is_none() {
                    this_obj.borrow_mut().prototype = Some(Rc::clone(&string_proto_clone));
                }
                Ok(Value::Object(this_obj))
            } else {
                Ok(Value::String(s))
            }
        },
        Rc::new(RefCell::new(Object::new(ObjectKind::Ordinary))),
    )))
}

fn create_string_constructor_object(
    string_proto: Rc<RefCell<Object>>,
    string_fn: Value,
) -> Rc<RefCell<Object>> {
    let string_obj = Object::new(ObjectKind::Ordinary);
    let string_obj_rc = Rc::new(RefCell::new(string_obj));
    string_obj_rc
        .borrow_mut()
        .set("prototype", Value::Object(string_proto));
    string_obj_rc
        .borrow_mut()
        .set("fromCharCode", create_from_char_code_fn());
    string_obj_rc
        .borrow_mut()
        .set("fromCodePoint", create_from_code_point_fn());
    string_obj_rc.borrow_mut().set("constructor", string_fn);
    string_obj_rc
}

fn create_from_char_code_fn() -> Value {
    Value::NativeFunction(Rc::new(NativeFunction::new(|args| {
        let chars: String = args
            .iter()
            .map(|v| {
                let code = crate::value::to_number(v) as u16;
                std::char::from_u32(code as u32).unwrap_or('\u{FFFD}')
            })
            .collect();
        Ok(Value::String(chars))
    })))
}

fn create_from_code_point_fn() -> Value {
    Value::NativeFunction(Rc::new(NativeFunction::new(|args| {
        let chars: String = args
            .iter()
            .map(|v| {
                let code = crate::value::to_number(v) as u32;
                std::char::from_u32(code).unwrap_or('\u{FFFD}')
            })
            .collect();
        Ok(Value::String(chars))
    })))
}

fn register_boolean_converter(ctx: &mut Context) {
    let boolean_proto = Object::new(ObjectKind::Ordinary);
    let boolean_proto_rc = Rc::new(RefCell::new(boolean_proto));
    if let Some(object_proto) = crate::builtins::get_object_prototype() {
        boolean_proto_rc.borrow_mut().prototype = Some(object_proto);
    }

    boolean_proto_rc.borrow_mut().set(
        "valueOf",
        Value::NativeFunction(Rc::new(NativeFunction::new(|_args| {
            let this_val = crate::builtins::get_native_this().unwrap_or(Value::Undefined);
            if let Value::Object(obj) = &this_val {
                if let Some(Value::Boolean(b)) = obj.borrow().get("_value") {
                    return Ok(Value::Boolean(b));
                }
            }
            match this_val {
                Value::Boolean(b) => Ok(Value::Boolean(b)),
                _ => Err(crate::JsError::new(
                    "TypeError: Boolean.prototype.valueOf requires a Boolean receiver",
                )),
            }
        }))),
    );

    let boolean_proto_clone = Rc::clone(&boolean_proto_rc);
    let boolean_fn = Value::NativeFunction(Rc::new(NativeFunction::new_with_prototype(
        move |args| {
            let b = args.first().map(to_bool).unwrap_or(false);
            let this_val = crate::builtins::get_native_this().unwrap_or(Value::Undefined);
            if let Value::Object(this_obj) = this_val {
                crate::builtins::object::set_boxed_value(
                    &mut this_obj.borrow_mut(),
                    Value::Boolean(b),
                );
                this_obj.borrow_mut().exotic_kind = Some(crate::value::kind::ExoticKind::Boolean);
                if this_obj.borrow().prototype.is_none() {
                    this_obj.borrow_mut().prototype = Some(Rc::clone(&boolean_proto_clone));
                }
                Ok(Value::Object(this_obj))
            } else {
                Ok(Value::Boolean(b))
            }
        },
        boolean_proto_rc.clone(),
    )));

    let boolean_obj = Object::new(ObjectKind::Ordinary);
    let boolean_obj_rc = Rc::new(RefCell::new(boolean_obj));
    boolean_obj_rc
        .borrow_mut()
        .set("prototype", Value::Object(boolean_proto_rc));
    boolean_obj_rc.borrow_mut().set("constructor", boolean_fn);
    ctx.set_global("Boolean".to_string(), Value::Object(boolean_obj_rc));
}

// ============================================================================
// Date
// ============================================================================

fn chrono_now() -> i64 {
    use std::time::{SystemTime, UNIX_EPOCH};
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_millis() as i64
}

/// Convert calendar date to Unix timestamp (seconds since epoch)
fn chrono_to_timestamp(year: i32, month: i32, day: i32, hour: i32, min: i32, sec: i32) -> i64 {
    let days = days_from_ymd(year, month, day);
    (days * 86400) + (hour as i64 * 3600) + (min as i64 * 60) + sec as i64
}

/// Calculate days from Unix epoch (1970-01-01) to given date.
/// Handles years before 1970 (negative day counts) and normalizes
/// out-of-range months (e.g. month 13 rolls into the next year).
/// Out-of-range days carry arithmetically into adjacent months.
fn days_from_ymd(year: i32, month: i32, day: i32) -> i64 {
    // Normalize months outside 1..=12 into the year
    let total_months = year as i64 * 12 + (month as i64 - 1);
    let year = total_months.div_euclid(12) as i32;
    let month = total_months.rem_euclid(12) as i32 + 1;

    let mut days = 0i64;
    if year >= 1970 {
        for y in 1970..year {
            days += if is_leap_year(y) { 366 } else { 365 };
        }
    } else {
        for y in year..1970 {
            days -= if is_leap_year(y) { 366 } else { 365 };
        }
    }
    for m in 1..month {
        days += days_in_month(year, m);
    }
    days + (day - 1) as i64 // day is 1-based, epoch is day 0
}

fn is_leap_year(year: i32) -> bool {
    (year % 4 == 0 && year % 100 != 0) || year % 400 == 0
}

fn days_in_month(year: i32, month: i32) -> i64 {
    match month {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        4 | 6 | 9 | 11 => 30,
        2 => {
            if is_leap_year(year) {
                29
            } else {
                28
            }
        }
        _ => 30,
    }
}

pub fn register_date(ctx: &mut Context) {
    let date_proto = Object::new(ObjectKind::Date);
    let date_proto_rc = Rc::new(RefCell::new(date_proto));

    date_proto_rc.borrow_mut().set(
        "toString",
        Value::NativeFunction(Rc::new(NativeFunction::new(|_args| {
            Ok(Value::String(format!("Date @ {}", chrono_now())))
        }))),
    );
    date_proto_rc.borrow_mut().set(
        "valueOf",
        Value::NativeFunction(Rc::new(NativeFunction::new(|_args| {
            let this_val = crate::builtins::get_native_this().unwrap_or(Value::Undefined);
            if let Value::Object(obj_rc) = this_val {
                if let Some(ts) = obj_rc.borrow().get("_timestamp") {
                    return Ok(ts);
                }
            }
            Ok(Value::Number(chrono_now() as f64))
        }))),
    );
    date_proto_rc.borrow_mut().set(
        "getTimezoneOffset",
        Value::NativeFunction(Rc::new(NativeFunction::new(|_args| {
            // Return local timezone offset in minutes (negative for ahead of UTC)
            // For simplicity, return 0 (UTC) - proper impl would check local timezone
            Ok(Value::Number(0.0))
        }))),
    );
    date_proto_rc.borrow_mut().set(
        "getTime",
        Value::NativeFunction(Rc::new(NativeFunction::new(|_args| {
            let this_val = crate::builtins::get_native_this().unwrap_or(Value::Undefined);
            if let Value::Object(obj_rc) = this_val {
                if let Some(ts) = obj_rc.borrow().get("_timestamp") {
                    return Ok(ts);
                }
            }
            Ok(Value::Number(chrono_now() as f64))
        }))),
    );
    if let Some(object_proto) = crate::builtins::get_object_prototype() {
        date_proto_rc.borrow_mut().prototype = Some(object_proto);
    }

    let date_proto_clone = Rc::clone(&date_proto_rc);
    let date_constructor = NativeConstructor::new(
        move |args| {
            let timestamp = if args.is_empty() {
                chrono_now() as f64
            } else if args.len() == 1 {
                crate::value::to_number(&args[0])
            } else {
                // new Date(year, month, day, hour, min, sec, ms)
                let year = crate::value::to_number(&args[0]) as i32;
                let month = crate::value::to_number(&args[1]) as i32;
                let day = args
                    .get(2)
                    .map(|v| crate::value::to_number(v) as i32)
                    .unwrap_or(1);
                let hour = args
                    .get(3)
                    .map(|v| crate::value::to_number(v) as i32)
                    .unwrap_or(0);
                let min = args
                    .get(4)
                    .map(|v| crate::value::to_number(v) as i32)
                    .unwrap_or(0);
                let sec = args
                    .get(5)
                    .map(|v| crate::value::to_number(v) as i32)
                    .unwrap_or(0);
                let ms = args
                    .get(6)
                    .map(|v| crate::value::to_number(v) as i32)
                    .unwrap_or(0);

                // Convert Y/M/D/H/M/S/MS to Unix timestamp
                // JS months are 0-based, but our helpers expect 1-based
                let total_secs = chrono_to_timestamp(year, month + 1, day, hour, min, sec);
                (total_secs * 1000) as f64 + ms as f64
            };

            let this_val = crate::builtins::get_native_this().unwrap_or(Value::Undefined);
            if let Value::Object(obj_rc) = this_val {
                obj_rc.borrow_mut().prototype = Some(Rc::clone(&date_proto_clone));
                obj_rc.borrow_mut().kind = ObjectKind::Date;
                obj_rc
                    .borrow_mut()
                    .set("_timestamp", Value::Number(timestamp));
                Ok(Value::Object(obj_rc))
            } else {
                let date_obj =
                    Object::with_prototype(ObjectKind::Date, Rc::clone(&date_proto_clone));
                let date = Rc::new(RefCell::new(date_obj));
                date.borrow_mut()
                    .set("_timestamp", Value::Number(timestamp));
                Ok(Value::Object(date))
            }
        },
        date_proto_rc.clone(),
    );

    let date_wrapper = Object::new(ObjectKind::Ordinary);
    let date_wrapper_rc = Rc::new(RefCell::new(date_wrapper));
    date_wrapper_rc.borrow_mut().set(
        "constructor",
        Value::NativeConstructor(Rc::new(date_constructor)),
    );
    date_wrapper_rc
        .borrow_mut()
        .set("prototype", Value::Object(Rc::clone(&date_proto_rc)));
    date_wrapper_rc.borrow_mut().set(
        "now",
        Value::NativeFunction(Rc::new(NativeFunction::new(|_args| {
            Ok(Value::Number(chrono_now() as f64))
        }))),
    );
    ctx.set_global("Date".to_string(), Value::Object(date_wrapper_rc));
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Context;
    use std::f64::consts::PI;

    #[test]
    fn test_days_from_ymd_before_1970_is_negative() {
        // 1969-01-01 is 365 days before the epoch
        assert_eq!(days_from_ymd(1969, 1, 1), -365);
        // 1968-01-01 includes leap year 1968
        assert_eq!(days_from_ymd(1968, 1, 1), -(365 + 366));
        // Epoch itself is day 0
        assert_eq!(days_from_ymd(1970, 1, 1), 0);
    }

    #[test]
    fn test_days_from_ymd_normalizes_month_overflow() {
        // Month 14 of 2024 == February 2025
        assert_eq!(days_from_ymd(2024, 14, 1), days_from_ymd(2025, 2, 1));
        // Month 0 rolls back to December of the previous year
        assert_eq!(days_from_ymd(2024, 0, 1), days_from_ymd(2023, 12, 1));
    }

    #[test]
    fn test_date_before_1970_has_negative_timestamp() {
        let mut ctx = Context::new().unwrap();
        let result = ctx.eval("new Date(1969, 0, 1).getTime()").unwrap();
        match result {
            Value::Number(n) => assert!(n < 0.0, "Date(1969,0,1).getTime() must be < 0, got {}", n),
            other => panic!("expected Number, got {:?}", other),
        }
    }

    #[test]
    fn test_date_month_overflow_normalizes() {
        let mut ctx = Context::new().unwrap();
        let overflow = ctx.eval("new Date(2024, 13, 1).getTime()").unwrap();
        let expected = ctx.eval("new Date(2025, 1, 1).getTime()").unwrap();
        assert_eq!(overflow, expected);
    }

    fn eval_num(src: &str) -> f64 {
        let mut ctx = Context::new().unwrap();
        match ctx.eval(src).unwrap() {
            Value::Number(n) => n,
            other => panic!("expected Number from {:?}, got {:?}", src, other),
        }
    }

    #[test]
    fn test_parse_float_accepts_infinity_literal() {
        // Per ECMA-262 StrNumericLiteral, parseFloat accepts the literal
        // "Infinity" (case-sensitive — only the exact capital-I spelling)
        // after the optional sign.
        assert!(eval_num("parseFloat(Infinity)").is_infinite());
        assert!(eval_num("parseFloat(Infinity)") > 0.0);
        assert!(eval_num("parseFloat(-Infinity)") < 0.0);
        assert!(eval_num("parseFloat('Infinity')").is_infinite());
        assert!(eval_num("parseFloat('-Infinity')").is_infinite());
        assert!(eval_num("parseFloat('-Infinity')") < 0.0);
        // Case-sensitive: "infinity" must be NaN.
        assert!(eval_num("parseFloat('infinity')").is_nan());
    }

    #[test]
    fn test_parse_float_decimal_then_exponent() {
        // ".01e+2" must round-trip to the same value as the literal .01e+2.
        // The per-digit scale*=0.1 accumulator loses precision and gives 10.
        assert_eq!(eval_num("parseFloat('.01e+2')"), 1.0);
        assert_eq!(eval_num("parseFloat('.5e1')"), 5.0);
        assert_eq!(eval_num("parseFloat('3.14')"), PI);
        assert_eq!(eval_num("parseFloat('.01')"), 0.01);
    }
}
