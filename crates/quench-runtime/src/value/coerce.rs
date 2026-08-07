//! Value coercion operations — to_js_string, to_bool, to_number, to_uint32.

use std::rc::Rc;

use crate::eval::call_value_with_this;
use crate::value::{JsError, Value};

// ─── to_js_string ───────────────────────────────────────────────────────────

/// Convert a Value to its JavaScript string representation
pub fn to_js_string(v: &Value) -> String {
    if let Some(s) = simple_string_value(v) {
        return s;
    }
    match v {
        Value::Object(o) => {
            let obj = o.borrow();
            if let Some(
                Value::Function(_) | Value::NativeFunction(_) | Value::NativeConstructor(_),
            ) = obj.get("toString")
            {
                let o_clone = Rc::clone(o);
                drop(obj);
                let to_string_val = o_clone.borrow().get("toString").unwrap();
                if let Ok(result) =
                    call_value_with_this(to_string_val, vec![], Value::Object(Rc::clone(&o_clone)))
                {
                    if let Some(s) = simple_string_value(&result) {
                        return s;
                    }
                }
                let fallback = o_clone
                    .borrow()
                    .get("message")
                    .and_then(|v| simple_string_value(&v))
                    .unwrap_or_else(|| "[object Object]".to_string());
                return fallback;
            }
            object_to_js_string(&obj)
        }
        Value::Function(f) => f.source_text(),
        Value::NativeFunction(_)
        | Value::NativeConstructor(_)
        | Value::Generator(_)
        | Value::Class(_) => "[Function]".to_string(),
        Value::Symbol(s) => format!("Symbol({})", s.desc.as_deref().unwrap_or("")),
        _ => "undefined".to_string(),
    }
}

pub fn simple_string_value(v: &Value) -> Option<String> {
    match v {
        Value::Undefined => Some("undefined".to_string()),
        Value::Null => Some("null".to_string()),
        Value::Boolean(b) => Some(b.to_string()),
        Value::Number(n) => Some(number_to_string(*n)),
        Value::String(s) => Some(s.clone()),
        Value::BigInt(bi) => Some(format!("{}n", bi)),
        _ => None,
    }
}

fn object_to_js_string(o: &crate::value::object::Object) -> String {
    match o.kind {
        crate::value::kind::ObjectKind::Array => {
            let parts: Vec<String> = o.elements.iter().map(to_js_string).collect();
            format!("[{}]", parts.join(","))
        }
        crate::value::kind::ObjectKind::Function => "[Function]".to_string(),
        _ => {
            let name = o
                .get("name")
                .and_then(|v| simple_string_value(&v))
                .unwrap_or_default();
            let msg = o
                .get("message")
                .and_then(|v| simple_string_value(&v))
                .unwrap_or_default();
            if name.is_empty() && msg.is_empty() {
                "[object Object]".to_string()
            } else if name.is_empty() {
                msg
            } else if msg.is_empty() {
                name
            } else {
                format!("{}: {}", name, msg)
            }
        }
    }
}

/// Convert a number to its canonical JS string representation.
/// This matches ES spec `Number::toString` (shortest representation).
pub fn number_to_string(n: f64) -> String {
    if n.is_nan() {
        return "NaN".to_string();
    }
    if !n.is_finite() {
        return if n == f64::INFINITY {
            "Infinity".to_string()
        } else {
            "-Infinity".to_string()
        };
    }
    if n == 0.0 {
        return "0".to_string();
    }
    let abs = n.abs();
    // Use exponential notation for very small or very large numbers.
    if !(1e-6..1e21).contains(&abs) {
        return canonical_exponential(n);
    }
    // For normal numbers, find the shortest representation.
    for prec in 1..=21 {
        let formatted = format!("{:.prec$}", n);
        if let Ok(parsed) = formatted.parse::<f64>() {
            if parsed == n && !has_redundant_decimal(&formatted) {
                return strip_trailing_zeros(&formatted);
            }
        }
    }
    n.to_string()
}

/// Format a number using exponential notation with the shortest significant digits.
fn canonical_exponential(n: f64) -> String {
    for prec in 1..=20 {
        let exp_str = format!("{:.prec$e}", n);
        // Parse back to verify it round-trips
        if let Ok(parsed) = exp_str.parse::<f64>() {
            if parsed == n {
                return strip_exponential_trailing_zeros(&exp_str);
            }
        }
    }
    format!("{:e}", n)
}

fn has_redundant_decimal(s: &str) -> bool {
    s.contains('.') && s.ends_with('0')
}

fn strip_trailing_zeros(s: &str) -> String {
    if let Some(pos) = s.rfind('.') {
        let without_trailing = s[..pos + 1].to_string() + s[pos + 1..].trim_end_matches('0');
        if without_trailing.ends_with('.') {
            without_trailing.trim_end_matches('.').to_string()
        } else {
            without_trailing
        }
    } else {
        s.to_string()
    }
}

fn strip_exponential_trailing_zeros(s: &str) -> String {
    if let Some(e_pos) = s.find('e') {
        let (mantissa, exp) = s.split_at(e_pos);
        let clean_mantissa = strip_trailing_zeros(mantissa);
        let clean_exp = exp.replace(['+'], "");
        format!("{}{}", clean_mantissa, clean_exp)
    } else {
        strip_trailing_zeros(s)
    }
}

// ─── to_bool ────────────────────────────────────────────────────────────────

/// Convert a Value to boolean (JavaScript truthiness)
pub fn to_bool(v: &Value) -> bool {
    match v {
        Value::Undefined | Value::Null => false,
        Value::Boolean(b) => *b,
        Value::Number(n) => *n != 0.0 && !n.is_nan(),
        Value::String(s) => !s.is_empty(),
        Value::Object(_)
        | Value::Function(_)
        | Value::NativeFunction(_)
        | Value::NativeConstructor(_)
        | Value::Generator(_)
        | Value::Class(_)
        | Value::Symbol(_) => true,
        Value::BigInt(bi) => **bi != num_bigint::BigInt::from(0i64),
    }
}

// ─── to_number ──────────────────────────────────────────────────────────────

/// Convert a Value to a number (JavaScript coercion)
pub fn to_number(v: &Value) -> f64 {
    if let Some(n) = simple_number_value(v) {
        return n;
    }
    to_number_complex(v).unwrap_or(f64::NAN)
}

/// Convert a Value to a number, propagating errors (for cases like isFinite that need to throw)
pub fn try_to_number(v: &Value) -> Result<f64, JsError> {
    if let Some(n) = simple_number_value(v) {
        return Ok(n);
    }
    to_number_complex(v)
}

/// Convert a Value to a number without error handling (assumes already validated).
pub fn to_number_unchecked(v: &Value) -> f64 {
    match v {
        Value::Number(n) => *n,
        Value::Boolean(b) => {
            if *b {
                1.0
            } else {
                0.0
            }
        }
        Value::String(s) => string_to_number(s),
        Value::Null => 0.0,
        _ => f64::NAN,
    }
}

fn simple_number_value(v: &Value) -> Option<f64> {
    match v {
        Value::Undefined => Some(f64::NAN),
        Value::Null => Some(0.0),
        Value::Boolean(b) => Some(if *b { 1.0 } else { 0.0 }),
        Value::Number(n) => Some(*n),
        Value::String(s) => Some(string_to_number(s)),
        _ => None,
    }
}

fn to_number_complex(v: &Value) -> Result<f64, JsError> {
    match v {
        Value::Object(_)
        | Value::Function(_)
        | Value::NativeFunction(_)
        | Value::NativeConstructor(_)
        | Value::Generator(_)
        | Value::Class(_) => Ok(to_number(&crate::value::to_primitive(v, Some("number"))?)),
        Value::Symbol(_) => {
            let (err_val, err) = crate::value::error::create_js_error_with_type(
                "Cannot convert a Symbol to a number",
                "TypeError",
            );
            crate::value::set_thrown_value(err_val);
            Err(err)
        }
        _ => Ok(f64::NAN),
    }
}

fn string_to_number(s: &str) -> f64 {
    let s = s.trim();
    if s.is_empty() {
        return 0.0;
    }
    if s == "Infinity" {
        return f64::INFINITY;
    }
    if s == "-Infinity" {
        return f64::NEG_INFINITY;
    }
    if s == "NaN" {
        return f64::NAN;
    }
    // Per ES §7.1.4.1: ToNumber Applied to the String Type
    if let Some(rest) = s.strip_prefix("0x").or_else(|| s.strip_prefix("0X")) {
        return u64::from_str_radix(rest, 16)
            .ok()
            .map(|n| n as f64)
            .unwrap_or(f64::NAN);
    }
    if let Some(rest) = s.strip_prefix("0b").or_else(|| s.strip_prefix("0B")) {
        return u64::from_str_radix(rest, 2)
            .ok()
            .map(|n| n as f64)
            .unwrap_or(f64::NAN);
    }
    if let Some(rest) = s.strip_prefix("0o").or_else(|| s.strip_prefix("0O")) {
        return u64::from_str_radix(rest, 8)
            .ok()
            .map(|n| n as f64)
            .unwrap_or(f64::NAN);
    }
    s.parse().unwrap_or(f64::NAN)
}

// ─── to_uint32 ──────────────────────────────────────────────────────────────

/// Convert a number to uint32 (JavaScript ToUint32)
/// Handles edge cases: NaN→0, Infinity→0, fractional→truncated.
pub fn to_uint32(n: f64) -> u32 {
    if !n.is_finite() || n.abs() < 1.0 {
        return 0;
    }
    // ToInteger (per ES §7.1.5) truncates toward zero.
    let i = n.trunc() as i64;
    // mod 2^32
    (i.rem_euclid(1i64 << 32)) as u32
}

#[cfg(test)]
mod tests;
