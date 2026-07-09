//! Value conversion utilities - to_js_string, to_bool, to_number, etc.

use std::rc::Rc;

use crate::value::Value;

/// Convert a Value to its JavaScript string representation
pub fn to_js_string(v: &Value) -> String {
    match v {
        Value::Undefined => "undefined".to_string(),
        Value::Null => "null".to_string(),
        Value::Boolean(b) => b.to_string(),
        Value::Number(n) => number_to_string(*n),
        Value::String(s) => s.clone(),
        Value::Object(o) => {
            let o = o.borrow();
            match o.kind {
                crate::value::kind::ObjectKind::Array => {
                    let parts: Vec<String> = o.elements.iter().map(to_js_string).collect();
                    format!("[{}]", parts.join(","))
                }
                crate::value::kind::ObjectKind::Function => "[Function]".to_string(),
                _ => "[object Object]".to_string(),
            }
        }
        Value::ObjectId(_) => "[object Object]".to_string(),
        Value::Function(_) => "[Function]".to_string(),
        Value::NativeFunction(_) => "[Function]".to_string(),
        Value::NativeConstructor(_) => "[Function]".to_string(),
        Value::Symbol(s) => format!("Symbol({})", s),
    }
}

fn number_to_string(n: f64) -> String {
    if n.is_nan() {
        "NaN".to_string()
    } else if n == f64::INFINITY {
        "Infinity".to_string()
    } else if n == f64::NEG_INFINITY {
        "-Infinity".to_string()
    } else if n.fract() == 0.0 && n.abs() < 1e15 {
        format!("{:.0}", n)
    } else {
        n.to_string()
    }
}

/// Convert a Value to boolean (JavaScript truthiness)
pub fn to_bool(v: &Value) -> bool {
    match v {
        Value::Undefined | Value::Null => false,
        Value::Boolean(b) => *b,
        Value::Number(n) => *n != 0.0 && !n.is_nan(),
        Value::String(s) => !s.is_empty(),
        Value::Object(_) | Value::ObjectId(_) | Value::Function(_) |
        Value::NativeFunction(_) | Value::NativeConstructor(_) => true,
        Value::Symbol(_) => false,
    }
}

/// Convert a Value to a number (JavaScript coercion)
pub fn to_number(v: &Value) -> f64 {
    match v {
        Value::Undefined => f64::NAN,
        Value::Null => 0.0,
        Value::Boolean(true) => 1.0,
        Value::Boolean(false) => 0.0,
        Value::Number(n) => *n,
        Value::String(s) => string_to_number(s),
        _ => f64::NAN,
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
    s.parse().unwrap_or(f64::NAN)
}

/// Strict equality comparison
pub fn strict_eq(a: &Value, b: &Value) -> bool {
    match (a, b) {
        (Value::Undefined, Value::Undefined) => true,
        (Value::Null, Value::Null) => true,
        (Value::Boolean(ai), Value::Boolean(bi)) => ai == bi,
        (Value::Number(ai), Value::Number(bi)) => ai == bi,
        (Value::String(ai), Value::String(bi)) => ai == bi,
        (Value::Object(ai), Value::Object(bi)) => Rc::ptr_eq(ai, bi),
        (Value::ObjectId(ai), Value::ObjectId(bi)) => ai == bi,
        (Value::Function(ai), Value::Function(bi)) => Rc::ptr_eq(&ai.closure, &bi.closure),
        (Value::NativeFunction(ai), Value::NativeFunction(bi)) => {
            Rc::ptr_eq(&ai.0, &bi.0)
        }
        (Value::NativeConstructor(ai), Value::NativeConstructor(bi)) => {
            Rc::ptr_eq(ai.func_rc(), bi.func_rc())
        }
        _ => false,
    }
}

/// Loose equality comparison (==)
///
/// Implements the ECMAScript Abstract Equality Comparison algorithm.
pub fn loose_eq(a: &Value, b: &Value) -> bool {
    if std::mem::discriminant(a) == std::mem::discriminant(b) {
        return strict_eq(a, b);
    }
    if null_undefined_eq(a, b) {
        return true;
    }
    if let Some(result) = number_string_eq(a, b) {
        return result;
    }
    if let Some(result) = boolean_coercion_eq(a, b) {
        return result;
    }
    if objectid_primitive_eq(a, b) {
        return false;
    }
    object_vs_primitive_eq(a, b)
}

fn null_undefined_eq(a: &Value, b: &Value) -> bool {
    matches!((a, b), (Value::Undefined, Value::Null) | (Value::Null, Value::Undefined))
}

fn number_string_eq(a: &Value, b: &Value) -> Option<bool> {
    match (a, b) {
        (Value::Number(n), Value::String(s)) => {
            Some(parse_number_string(s).map_or(false, |p| *n == p))
        }
        (Value::String(s), Value::Number(n)) => {
            Some(parse_number_string(s).map_or(false, |p| p == *n))
        }
        _ => None,
    }
}

fn boolean_coercion_eq(a: &Value, b: &Value) -> Option<bool> {
    match (a, b) {
        (Value::Boolean(bv), other) => {
            Some(loose_eq(&Value::Number(bool_to_num(*bv)), other))
        }
        (other, Value::Boolean(bv)) => {
            Some(loose_eq(other, &Value::Number(bool_to_num(*bv))))
        }
        _ => None,
    }
}

fn bool_to_num(b: bool) -> f64 {
    if b { 1.0 } else { 0.0 }
}

fn objectid_primitive_eq(a: &Value, b: &Value) -> bool {
    match (a, b) {
        (Value::ObjectId(_), other) | (other, Value::ObjectId(_)) => {
            !matches!(other, Value::ObjectId(_) | Value::Object(_))
        }
        _ => false,
    }
}

fn object_vs_primitive_eq(a: &Value, b: &Value) -> bool {
    match (a, b) {
        (Value::Object(_), Value::Number(_) | Value::String(_)) => {
            loose_eq(&to_primitive_for_compare(a), b)
        }
        (Value::Number(_) | Value::String(_), Value::Object(_)) => {
            loose_eq(a, &to_primitive_for_compare(b))
        }
        (Value::Object(_), _) => loose_eq(&to_primitive_for_compare(a), b),
        (_, Value::Object(_)) => loose_eq(a, &to_primitive_for_compare(b)),
        _ => false,
    }
}

fn parse_number_string(s: &str) -> Option<f64> {
    let trimmed = s.trim();
    if trimmed.is_empty() {
        Some(0.0)
    } else {
        trimmed.parse::<f64>().ok()
    }
}

fn to_primitive_for_compare(v: &Value) -> Value {
    match v {
        Value::Undefined => Value::Undefined,
        Value::Null => Value::Null,
        Value::Boolean(b) => Value::Boolean(*b),
        Value::Number(n) => Value::Number(*n),
        Value::String(s) => Value::String(s.clone()),
        Value::Symbol(s) => Value::Symbol(s.clone()),
        Value::ObjectId(_) => Value::String("[object Object]".to_string()),
        Value::Object(obj) => {
            if let Some(method) = obj.borrow().get("valueOf") {
                if let Value::NativeFunction(nf) = method {
                    if let Ok(result) = nf.call(vec![]) {
                        if !matches!(result, Value::Object(_)) {
                            return result;
                        }
                    }
                }
            }
            if let Some(method) = obj.borrow().get("toString") {
                if let Value::NativeFunction(nf) = method {
                    if let Ok(result) = nf.call(vec![]) {
                        if !matches!(result, Value::Object(_)) {
                            return result;
                        }
                    }
                }
            }
            Value::String("[object Object]".to_string())
        }
        Value::Function(_) => Value::String("[object Function]".to_string()),
        Value::NativeFunction(_) => Value::String("[object Function]".to_string()),
        Value::NativeConstructor(_) => Value::String("[object Function]".to_string()),
    }
}

/// Hint for ToPrimitive conversion
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PrimitiveHint {
    Number,
    String,
}

/// Convert a Value to a primitive using JavaScript's ToPrimitive abstract operation.
pub fn to_primitive(value: &Value, hint: Option<&str>) -> Value {
    match value {
        Value::Undefined => Value::Undefined,
        Value::Null => Value::Null,
        Value::Boolean(b) => Value::Boolean(*b),
        Value::Number(n) => Value::Number(*n),
        Value::String(s) => Value::String(s.clone()),
        Value::Symbol(s) => Value::Symbol(s.clone()),
        Value::Object(obj) => to_primitive_object(&obj.borrow(), hint),
        Value::ObjectId(_) => Value::String("[object Object]".to_string()),
        Value::Function(_) | Value::NativeFunction(_) | Value::NativeConstructor(_) => {
            Value::String("[Function]".to_string())
        }
    }
}

fn to_primitive_object(obj: &crate::value::object::Object, hint: Option<&str>) -> Value {
    let hint = match hint {
        Some("string") => PrimitiveHint::String,
        Some("number") | None => PrimitiveHint::Number,
        _ => PrimitiveHint::Number,
    };

    let (first, second) = match hint {
        PrimitiveHint::Number => ("valueOf", "toString"),
        PrimitiveHint::String => ("toString", "valueOf"),
    };

    if let Some(result) = try_method(obj, first) {
        return result;
    }
    if let Some(result) = try_method(obj, second) {
        return result;
    }

    Value::String(to_js_string(&Value::Object(std::rc::Rc::new(
        std::cell::RefCell::new(crate::value::object::Object::new(
            crate::value::kind::ObjectKind::Ordinary,
        )),
    ))))
}

fn try_method(obj: &crate::value::object::Object, method_name: &str) -> Option<Value> {
    let method = obj.get(method_name)?;
    if let Value::NativeFunction(nf) = &method {
        if let Ok(result) = nf.call(vec![]) {
            if !matches!(result, Value::Object(_)) {
                return Some(result);
            }
        }
    }
    None
}
