//! Equality and comparison operations — strict_eq, loose_eq, same_value.
//!
//! These are canonical spec ops that must stay in sync with ECMA-262.

use std::rc::Rc;

use crate::value::JsError;
use crate::value::Value;

/// ToPrimitive for comparison with strict (Result) return.
pub fn to_primitive_for_compare_strict(v: &Value) -> Result<Value, JsError> {
    match crate::value::to_primitive(v, None) {
        Ok(value) => Ok(value),
        Err(_error) if matches!(v, Value::Object(_)) => Ok(to_primitive_for_compare(v)),
        Err(error) => Err(error),
    }
}

// ─── strict_eq ──────────────────────────────────────────────────────────────

/// Strict equality comparison (===)
pub fn strict_eq(a: &Value, b: &Value) -> bool {
    let disc_a = std::mem::discriminant(a);
    let disc_b = std::mem::discriminant(b);
    if disc_a == disc_b {
        return strict_eq_same_type(a, b);
    }
    matches!(
        (a, b),
        (Value::Undefined, Value::Undefined) | (Value::Null, Value::Null)
    )
}

fn strict_eq_same_type(a: &Value, b: &Value) -> bool {
    match (a, b) {
        (Value::Undefined, Value::Undefined) | (Value::Null, Value::Null) => true,
        (Value::Boolean(ai), Value::Boolean(bi)) => ai == bi,
        (Value::Number(ai), Value::Number(bi)) => ai == bi,
        (Value::String(ai), Value::String(bi)) => ai == bi,
        (Value::Symbol(ai), Value::Symbol(bi)) => ai.id == bi.id,
        (Value::Object(ai), Value::Object(bi)) => Rc::ptr_eq(ai, bi),
        (Value::Generator(ai), Value::Generator(bi)) => Rc::ptr_eq(ai, bi),
        (Value::BigInt(ai), Value::BigInt(bi)) => ai.as_ref() == bi.as_ref(),
        (Value::Function(_), Value::Function(_))
        | (Value::NativeFunction(_), Value::NativeFunction(_))
        | (Value::NativeConstructor(_), Value::NativeConstructor(_))
        | (Value::Class(_), Value::Class(_)) => strict_eq_funcs(a, b),
        _ => false,
    }
}

fn strict_eq_funcs(a: &Value, b: &Value) -> bool {
    match (a, b) {
        (Value::Function(ai), Value::Function(bi)) => ai.identity_ptr() == bi.identity_ptr(),
        (Value::NativeFunction(ai), Value::NativeFunction(bi)) => ai == bi,
        (Value::NativeConstructor(ai), Value::NativeConstructor(bi)) => ai == bi,
        (Value::Class(ai), Value::Class(bi)) => ai.id == bi.id,
        _ => false,
    }
}

// ─── same_value ─────────────────────────────────────────────────────────────

/// SameValue comparison (ES2015+) — same as === but NaN equals NaN and +0 != -0
pub fn same_value(a: &Value, b: &Value) -> bool {
    if std::mem::discriminant(a) != std::mem::discriminant(b) {
        return false;
    }
    same_value_same_type(a, b)
}

/// SameValueZero (ES2025 §7.2.11): like SameValue, but `-0` and `+0` are equal.
pub fn same_value_zero(a: &Value, b: &Value) -> bool {
    if std::mem::discriminant(a) != std::mem::discriminant(b) {
        return false;
    }
    match (a, b) {
        (Value::Number(ai), Value::Number(bi)) => *ai == *bi || (ai.is_nan() && bi.is_nan()),
        (Value::Symbol(ai), Value::Symbol(bi)) => ai.id == bi.id,
        (Value::Object(ai), Value::Object(bi)) => Rc::ptr_eq(ai, bi),
        (Value::Function(_), Value::Function(_))
        | (Value::NativeFunction(_), Value::NativeFunction(_))
        | (Value::NativeConstructor(_), Value::NativeConstructor(_))
        | (Value::Class(_), Value::Class(_)) => strict_eq_funcs(a, b),
        (Value::Generator(ai), Value::Generator(bi)) => Rc::ptr_eq(ai, bi),
        _ => a == b,
    }
}

#[allow(clippy::complexity)]
fn same_value_same_type(a: &Value, b: &Value) -> bool {
    match (a, b) {
        (Value::Number(ai), Value::Number(bi)) => same_value_numbers(*ai, *bi),
        (Value::Function(_), Value::Function(_))
        | (Value::NativeFunction(_), Value::NativeFunction(_))
        | (Value::NativeConstructor(_), Value::NativeConstructor(_))
        | (Value::Class(_), Value::Class(_)) => strict_eq_funcs(a, b),
        (Value::Undefined, Value::Undefined) | (Value::Null, Value::Null) => true,
        (Value::Boolean(ai), Value::Boolean(bi)) => ai == bi,
        (Value::String(ai), Value::String(bi)) => ai == bi,
        (Value::Symbol(ai), Value::Symbol(bi)) => ai.id == bi.id,
        (Value::Object(ai), Value::Object(bi)) => Rc::ptr_eq(ai, bi),
        (Value::Generator(ai), Value::Generator(bi)) => Rc::ptr_eq(ai, bi),
        (Value::BigInt(ai), Value::BigInt(bi)) => ai.as_ref() == bi.as_ref(),
        _ => false,
    }
}

pub fn same_value_numbers(a: f64, b: f64) -> bool {
    if a.is_nan() && b.is_nan() {
        return true;
    }
    if a == b {
        if a == 0.0 {
            return (1.0f64 / a).is_sign_positive() == (1.0f64 / b).is_sign_positive();
        }
        return true;
    }
    false
}

// ─── loose_eq ───────────────────────────────────────────────────────────────

/// Loose equality comparison (==)
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
    if let Some(result) = bigint_string_eq(a, b) {
        return result;
    }
    if let Some(result) = bigint_number_eq(a, b) {
        return result;
    }
    if let Some(result) = boolean_coercion_eq(a, b) {
        return result;
    }
    object_vs_primitive_eq(a, b)
}

fn bigint_string_eq(a: &Value, b: &Value) -> Option<bool> {
    let (bigint, string) = match (a, b) {
        (Value::BigInt(value), Value::String(text)) => (value, text),
        (Value::String(text), Value::BigInt(value)) => (value, text),
        _ => return None,
    };
    let text = string.trim();
    let parsed = if text.is_empty() {
        num_bigint::BigInt::from(0)
    } else {
        match text.parse::<num_bigint::BigInt>() {
            Ok(value) => value,
            Err(_) => return Some(false),
        }
    };
    Some(parsed == bigint.as_ref().clone())
}

fn bigint_number_eq(a: &Value, b: &Value) -> Option<bool> {
    let (bigint, number) = match (a, b) {
        (Value::BigInt(value), Value::Number(number)) => (value, *number),
        (Value::Number(number), Value::BigInt(value)) => (value, *number),
        _ => return None,
    };
    if !number.is_finite() || number.fract() != 0.0 {
        return Some(false);
    }
    let text = format!("{number:.0}");
    let parsed = num_bigint::BigInt::parse_bytes(text.as_bytes(), 10)?;
    Some(parsed == bigint.as_ref().clone())
}

fn null_undefined_eq(a: &Value, b: &Value) -> bool {
    matches!(
        (a, b),
        (Value::Undefined, Value::Null) | (Value::Null, Value::Undefined)
    )
}

fn number_string_eq(a: &Value, b: &Value) -> Option<bool> {
    match (a, b) {
        (Value::Number(n), Value::String(s)) => Some(parse_number_string(s) == Some(*n)),
        (Value::String(s), Value::Number(n)) => Some(parse_number_string(s) == Some(*n)),
        _ => None,
    }
}

fn boolean_coercion_eq(a: &Value, b: &Value) -> Option<bool> {
    match (a, b) {
        (Value::Boolean(bv), other) => {
            Some(loose_eq(&Value::Number(if *bv { 1.0 } else { 0.0 }), other))
        }
        (other, Value::Boolean(bv)) => {
            Some(loose_eq(other, &Value::Number(if *bv { 1.0 } else { 0.0 })))
        }
        _ => None,
    }
}

pub fn parse_number_string(s: &str) -> Option<f64> {
    let trimmed = s.trim();
    if trimmed.is_empty() {
        return Some(0.0);
    }
    if let Some(rest) = trimmed
        .strip_prefix("0x")
        .or_else(|| trimmed.strip_prefix("0X"))
    {
        return parse_radix_number(rest, 16);
    }
    if let Some(rest) = trimmed
        .strip_prefix("0b")
        .or_else(|| trimmed.strip_prefix("0B"))
    {
        return parse_radix_number(rest, 2);
    }
    if let Some(rest) = trimmed
        .strip_prefix("0o")
        .or_else(|| trimmed.strip_prefix("0O"))
    {
        return parse_radix_number(rest, 8);
    }
    trimmed.parse::<f64>().ok()
}

fn parse_radix_number(source: &str, radix: u32) -> Option<f64> {
    if source.is_empty() {
        return None;
    }
    source.chars().try_fold(0.0, |value, character| {
        character
            .to_digit(radix)
            .map(|digit| value * radix as f64 + digit as f64)
    })
}

/// ToPrimitive for object comparison — returns Result so we can propagate
/// the TypeError when both valueOf and toString return non-primitive values.
#[allow(dead_code)]
pub fn to_primitive_for_compare(v: &Value) -> Value {
    if let Some(prim) = primitive_for_compare(v) {
        return prim;
    }
    match v {
        Value::Object(obj) => object_to_primitive_for_compare(obj),
        Value::Function(_)
        | Value::NativeFunction(_)
        | Value::NativeConstructor(_)
        | Value::Class(_) => Value::String("[object Function]".to_string()),
        _ => Value::Undefined,
    }
}

pub fn primitive_for_compare(v: &Value) -> Option<Value> {
    match v {
        Value::Undefined => Some(Value::Undefined),
        Value::Null => Some(Value::Null),
        Value::Boolean(b) => Some(Value::Boolean(*b)),
        Value::Number(n) => Some(Value::Number(*n)),
        Value::String(s) => Some(Value::String(s.clone())),
        Value::Symbol(s) => Some(Value::Symbol(s.clone())),
        Value::BigInt(bi) => Some(Value::BigInt(Rc::clone(bi))),
        _ => None,
    }
}

#[allow(dead_code)]
fn object_to_primitive_for_compare(obj: &Rc<std::cell::RefCell<crate::value::Object>>) -> Value {
    use crate::eval::function::call_value_with_this;
    let obj_borrowed = obj.borrow();
    let value_of = obj_borrowed.get("valueOf");
    let method = value_of.and_then(|m| match m {
        Value::NativeFunction(_) | Value::Function(_) => Some(m.clone()),
        _ => None,
    });
    drop(obj_borrowed);
    if let Some(method) = method {
        let this_val = Value::Object(Rc::clone(obj));
        if let Ok(result) = call_value_with_this(method, vec![], this_val) {
            if !matches!(result, Value::Object(_)) {
                return result;
            }
        }
    }
    let obj_borrowed = obj.borrow();
    let to_string = obj_borrowed.get("toString");
    let method = to_string.and_then(|m| match m {
        Value::NativeFunction(_) | Value::Function(_) => Some(m.clone()),
        _ => None,
    });
    drop(obj_borrowed);
    if let Some(method) = method {
        let this_val = Value::Object(Rc::clone(obj));
        if let Ok(result) = call_value_with_this(method, vec![], this_val) {
            if !matches!(result, Value::Object(_)) {
                return result;
            }
        }
    }
    Value::String("[object Object]".to_string())
}

fn object_vs_primitive_eq(a: &Value, b: &Value) -> bool {
    match (a, b) {
        (Value::Object(_), Value::Null | Value::Undefined)
        | (Value::Null | Value::Undefined, Value::Object(_)) => false,
        (Value::Object(_), Value::Number(_) | Value::String(_)) => {
            match to_primitive_for_compare_strict(a) {
                Ok(prim) => loose_eq(&prim, b),
                Err(_) => false,
            }
        }
        (Value::Number(_) | Value::String(_), Value::Object(_)) => {
            match to_primitive_for_compare_strict(b) {
                Ok(prim) => loose_eq(a, &prim),
                Err(_) => false,
            }
        }
        (Value::Object(_), _) => match to_primitive_for_compare_strict(a) {
            Ok(prim) => loose_eq(&prim, b),
            Err(_) => false,
        },
        (_, Value::Object(_)) => match to_primitive_for_compare_strict(b) {
            Ok(prim) => loose_eq(a, &prim),
            Err(_) => false,
        },
        _ => false,
    }
}

#[cfg(test)]
mod tests;
