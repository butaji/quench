//! Value conversion utilities - to_js_string, to_bool, to_number, etc.

use std::cell::RefCell;
use std::rc::Rc;

use crate::eval::call_value_with_this;
use crate::value::{JsError, Value};

/// Convert a Value to its JavaScript string representation
pub fn to_js_string(v: &Value) -> String {
    if let Some(s) = simple_string_value(v) {
        return s;
    }
    match v {
        Value::Object(o) => {
            let obj = o.borrow();
            // Try calling toString if it's a function (handles Error objects)
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
        Value::Function(_)
        | Value::NativeFunction(_)
        | Value::NativeConstructor(_)
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
            // Try to format as an Error: "ErrorName: message"
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

fn number_to_string(n: f64) -> String {
    if n.is_nan() {
        "NaN".to_string()
    } else if n == f64::INFINITY {
        "Infinity".to_string()
    } else if n == f64::NEG_INFINITY {
        "-Infinity".to_string()
    } else if n == 0.0 {
        // Per spec, both +0 and -0 stringify to "0".
        "0".to_string()
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
        Value::Object(_)
        | Value::Function(_)
        | Value::NativeFunction(_)
        | Value::NativeConstructor(_)
        | Value::Class(_)
        | Value::BigInt(_) => true,
        Value::Symbol(_) => true,
        Value::BigInt(_) => true,
    }
}

/// Convert a Value to a number (JavaScript coercion)
pub fn to_number(v: &Value) -> f64 {
    if let Some(n) = simple_number_value(v) {
        return n;
    }
    match to_number_complex(v) {
        Ok(n) => n,
        Err(_) => f64::NAN,
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
        | Value::Class(_) => Ok(to_number(&to_primitive(v, Some("number"))?)),
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
    // - Empty string → 0
    // - Numeric string with possible whitespace and/or sign
    // - Hex literal (0x...) or octal literal (0o...) or binary literal (0b...)
    // - Infinity / -Infinity / NaN
    // Note: Rust's f64::parse doesn't accept hex/octal/binary prefixes.
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

/// Convert a number to uint32 (JavaScript ToUint32)
/// Handles edge cases: NaN→0, Infinity→0, fractional→truncated.
/// To avoid FP imprecision at power-of-2 boundaries (e.g. -2147483649.1),
/// route through i64 math via modulo on the integer part.
pub fn to_uint32(n: f64) -> u32 {
    if !n.is_finite() || n.abs() < 1.0 {
        return 0;
    }
    // ToInteger (per ES §7.1.5) truncates toward zero.
    let i = n.trunc() as i64;
    // mod 2^32
    (i.rem_euclid(1i64 << 32)) as u32
}

/// Strict equality comparison
pub fn strict_eq(a: &Value, b: &Value) -> bool {
    if std::mem::discriminant(a) == std::mem::discriminant(b) {
        return strict_eq_same_type(a, b);
    }
    if null_undefined_strict_eq(a, b) {
        return true;
    }
    false
}

fn null_undefined_strict_eq(a: &Value, b: &Value) -> bool {
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
        (Value::Symbol(ai), Value::Symbol(bi)) => Rc::ptr_eq(ai, bi),
        (Value::Object(ai), Value::Object(bi)) => Rc::ptr_eq(ai, bi),
        (Value::Function(_), Value::Function(_))
        | (Value::NativeFunction(_), Value::NativeFunction(_))
        | (Value::NativeConstructor(_), Value::NativeConstructor(_))
        | (Value::Class(_), Value::Class(_)) => strict_eq_funcs(a, b),
        _ => false,
    }
}

fn strict_eq_funcs(a: &Value, b: &Value) -> bool {
    match (a, b) {
        // Function identity: each declaration gets its own proto cell at
        // construction; clones share it. (Comparing closure environments
        // would make distinct functions in the same scope compare equal.)
        (Value::Function(ai), Value::Function(bi)) => ai.identity_ptr() == bi.identity_ptr(),
        (Value::NativeFunction(ai), Value::NativeFunction(bi)) => Rc::ptr_eq(&ai.func, &bi.func),
        (Value::NativeConstructor(ai), Value::NativeConstructor(bi)) => {
            Rc::ptr_eq(ai.func_rc(), bi.func_rc())
        }
        // Class identity: ClassValue::id is assigned at construction and
        // preserved across the deep-copying Value::clone.
        (Value::Class(ai), Value::Class(bi)) => ai.id == bi.id,
        _ => false,
    }
}

/// SameValue comparison (ES2015+)
///
/// Implements ECMAScript SameValue algorithm:
/// - Same as === except NaN equals NaN and +0 != -0
pub fn same_value(a: &Value, b: &Value) -> bool {
    // SameValue does NOT equate null and undefined (unlike strict equality).
    // Only same-type values can be same value.
    if std::mem::discriminant(a) != std::mem::discriminant(b) {
        return false;
    }
    same_value_same_type(a, b)
}

#[allow(clippy::complexity)]
fn same_value_same_type(a: &Value, b: &Value) -> bool {
    // Fast path for primitives that need special handling
    match (a, b) {
        (Value::Number(ai), Value::Number(bi)) => return same_value_numbers(*ai, *bi),
        (Value::Function(_), Value::Function(_))
        | (Value::NativeFunction(_), Value::NativeFunction(_))
        | (Value::NativeConstructor(_), Value::NativeConstructor(_))
        | (Value::Class(_), Value::Class(_)) => return strict_eq_funcs(a, b),
        _ => {}
    }
    // Simple equality for rest
    match (a, b) {
        (Value::Undefined, Value::Undefined) | (Value::Null, Value::Null) => true,
        (Value::Boolean(ai), Value::Boolean(bi)) => ai == bi,
        (Value::String(ai), Value::String(bi)) => ai == bi,
        (Value::Symbol(ai), Value::Symbol(bi)) => Rc::ptr_eq(ai, bi),
        (Value::Object(ai), Value::Object(bi)) => Rc::ptr_eq(ai, bi),
        _ => false,
    }
}

fn same_value_numbers(a: f64, b: f64) -> bool {
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
    object_vs_primitive_eq(a, b)
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
        (Value::Boolean(bv), other) => Some(loose_eq(&Value::Number(bool_to_num(*bv)), other)),
        (other, Value::Boolean(bv)) => Some(loose_eq(other, &Value::Number(bool_to_num(*bv)))),
        _ => None,
    }
}

fn bool_to_num(b: bool) -> f64 {
    if b {
        1.0
    } else {
        0.0
    }
}

fn object_vs_primitive_eq(a: &Value, b: &Value) -> bool {
    match (a, b) {
        (Value::Object(_), Value::Number(_) | Value::String(_)) => {
            match to_primitive_for_compare_strict(a) {
                Ok(prim) => loose_eq(&prim, b),
                Err(_) => return false, // throw propagates via thrown_value; comparison returns false here
            }
        }
        (Value::Number(_) | Value::String(_), Value::Object(_)) => {
            match to_primitive_for_compare_strict(b) {
                Ok(prim) => loose_eq(a, &prim),
                Err(_) => return false,
            }
        }
        (Value::Object(_), _) => match to_primitive_for_compare_strict(a) {
            Ok(prim) => loose_eq(&prim, b),
            Err(_) => return false,
        },
        (_, Value::Object(_)) => match to_primitive_for_compare_strict(b) {
            Ok(prim) => loose_eq(a, &prim),
            Err(_) => return false,
        },
        _ => false,
    }
}

fn parse_number_string(s: &str) -> Option<f64> {
    let trimmed = s.trim();
    if trimmed.is_empty() {
        return Some(0.0);
    }
    if let Some(rest) = trimmed
        .strip_prefix("0x")
        .or_else(|| trimmed.strip_prefix("0X"))
    {
        return u64::from_str_radix(rest, 16).ok().map(|n| n as f64);
    }
    if let Some(rest) = trimmed
        .strip_prefix("0b")
        .or_else(|| trimmed.strip_prefix("0B"))
    {
        return u64::from_str_radix(rest, 2).ok().map(|n| n as f64);
    }
    if let Some(rest) = trimmed
        .strip_prefix("0o")
        .or_else(|| trimmed.strip_prefix("0O"))
    {
        return u64::from_str_radix(rest, 8).ok().map(|n| n as f64);
    }
    trimmed.parse::<f64>().ok()
}

fn to_primitive_for_compare(v: &Value) -> Value {
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

/// ToPrimitive for object comparison — returns Result so we can propagate
/// the TypeError when both valueOf and toString return non-primitive values.
fn to_primitive_for_compare_strict(v: &Value) -> Result<Value, JsError> {
    if let Some(prim) = primitive_for_compare(v) {
        return Ok(prim);
    }
    match v {
        Value::Object(_) => to_primitive(v, Some("number")),
        Value::Function(_)
        | Value::NativeFunction(_)
        | Value::NativeConstructor(_)
        | Value::Class(_) => Ok(Value::String("[object Function]".to_string())),
        _ => Ok(Value::Undefined),
    }
}

fn primitive_for_compare(v: &Value) -> Option<Value> {
    match v {
        Value::Undefined => Some(Value::Undefined),
        Value::Null => Some(Value::Null),
        Value::Boolean(b) => Some(Value::Boolean(*b)),
        Value::Number(n) => Some(Value::Number(*n)),
        Value::String(s) => Some(Value::String(s.clone())),
        Value::Symbol(s) => Some(Value::Symbol(s.clone())),
        _ => None,
    }
}

fn object_to_primitive_for_compare(obj: &Rc<std::cell::RefCell<crate::value::Object>>) -> Value {
    let obj_borrowed = obj.borrow();
    // Try valueOf first — handle NativeFunction OR Function (JS-defined).
    let value_of = obj_borrowed.get("valueOf");
    let method = value_of.and_then(|m| match m {
        Value::NativeFunction(_) | Value::Function(_) => Some(m.clone()),
        _ => None,
    });
    drop(obj_borrowed);
    if let Some(method) = method {
        let this_val = Value::Object(Rc::clone(obj));
        if let Ok(result) = crate::eval::function::call_value_with_this(method, vec![], this_val) {
            if !matches!(result, Value::Object(_)) {
                return result;
            }
        }
    }
    // Then toString.
    let obj_borrowed = obj.borrow();
    let to_string = obj_borrowed.get("toString");
    let method = to_string.and_then(|m| match m {
        Value::NativeFunction(_) | Value::Function(_) => Some(m.clone()),
        _ => None,
    });
    drop(obj_borrowed);
    if let Some(method) = method {
        let this_val = Value::Object(Rc::clone(obj));
        if let Ok(result) = crate::eval::function::call_value_with_this(method, vec![], this_val) {
            if !matches!(result, Value::Object(_)) {
                return result;
            }
        }
    }
    Value::String("[object Object]".to_string())
}

/// Hint for ToPrimitive conversion
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PrimitiveHint {
    Default,
    Number,
    String,
}

/// Convert a Value to a primitive using JavaScript's ToPrimitive abstract operation.
pub fn to_primitive(value: &Value, hint: Option<&str>) -> Result<Value, JsError> {
    if let Some(prim) = primitive_direct(value) {
        return Ok(prim);
    }
    match value {
        Value::Object(obj) => to_primitive_object(obj, hint),
        Value::Function(f) => to_primitive_function(&std::rc::Rc::new(f.clone()), hint),
        Value::NativeFunction(_) | Value::NativeConstructor(_) | Value::Class(_) => {
            Ok(Value::String("[Function]".to_string()))
        }
        _ => Ok(Value::Undefined),
    }
}

/// ToPrimitive for a user-defined JS Function. JS functions inherit
/// valueOf/toString from Object.prototype, but calling those on a function
/// recurses (valueOf returns `this`, toString returns "[object Function]").
/// We only honour OWN properties (e.g. `f.valueOf = function() { return 1 }`).
/// Inherited methods fall back to a textual representation.
fn to_primitive_function(
    f: &Rc<crate::value::function::ValueFunction>,
    hint: Option<&str>,
) -> Result<Value, JsError> {
    let hint = resolve_hint(hint);

    let (first, second) = match hint {
        PrimitiveHint::Default | PrimitiveHint::Number => ("valueOf", "toString"),
        PrimitiveHint::String => ("toString", "valueOf"),
    };

    // Only check OWN properties — walking the prototype chain and calling
    // Object.prototype.valueOf/toString on a function recurses infinitely.
    let first_method = f.get_property(first);
    let second_method = f.get_property(second);

    let this_val = Value::Function((**f).clone());

    let mut first_primitive: Option<Value> = None;
    let mut first_was_object = false;
    if let Some(m) = first_method.clone() {
        match crate::eval::call_value_with_this(m, vec![], this_val.clone()) {
            Ok(v) => {
                if !matches!(v, Value::Object(_)) {
                    return Ok(v);
                }
                first_was_object = true;
                first_primitive = Some(v);
            }
            Err(_) => {
                return Err(crate::value::error::create_js_error_with_type(
                    "valueOf/toString threw",
                    "Error",
                )
                .1)
            }
        }
    }
    if let Some(m) = second_method.clone() {
        match crate::eval::call_value_with_this(m, vec![], this_val.clone()) {
            Ok(v) => {
                if !matches!(v, Value::Object(_)) {
                    return Ok(v);
                }
                if first_was_object {
                    // Both returned Object -> TypeError
                    let (err, _) = crate::value::create_js_error_with_type(
                        "Cannot convert object to primitive value",
                        "TypeError",
                    );
                    crate::value::set_thrown_value(err);
                    return Err(crate::value::JsError("TypeError".to_string()));
                }
            }
            Err(_) => {
                return Err(crate::value::error::create_js_error_with_type(
                    "valueOf/toString threw",
                    "Error",
                )
                .1)
            }
        }
    }
    let _ = first_primitive; // suppress unused warning
                             // Fallback: match to_js_string's representation for Value::Function so that
                             // `f + ""` and `f.toString() + ""` agree.
    Ok(Value::String("[Function]".to_string()))
}

fn primitive_direct(v: &Value) -> Option<Value> {
    match v {
        Value::Undefined => Some(Value::Undefined),
        Value::Null => Some(Value::Null),
        Value::Boolean(b) => Some(Value::Boolean(*b)),
        Value::Number(n) => Some(Value::Number(*n)),
        Value::String(s) => Some(Value::String(s.clone())),
        Value::Symbol(s) => Some(Value::Symbol(s.clone())),
        _ => None,
    }
}

fn to_primitive_object(
    obj: &Rc<RefCell<crate::value::object::Object>>,
    hint: Option<&str>,
) -> Result<Value, JsError> {
    let hint = resolve_hint(hint);

    // Check Symbol.toPrimitive first. Returns:
    // - Ok(Some(v)) when @@toPrimitive produced a primitive
    // - Ok(None) when no @@toPrimitive exists or it returned an object
    // - Err(_) when @@toPrimitive threw
    match try_to_primitive_symbol(obj, hint)? {
        Some(v) => return Ok(v),
        None => {}
    }

    // Try valueOf then toString (or vice versa for string hint)
    let (first, second) = match hint {
        PrimitiveHint::Default | PrimitiveHint::Number => ("valueOf", "toString"),
        PrimitiveHint::String => ("toString", "valueOf"),
    };

    // Per ES spec: if valueOf throws, the throw propagates — we must NOT fall
    // through to toString.
    // Track whether either method was callable. If both methods exist but
    // return non-primitives, ToPrimitive must throw TypeError per ES §7.1.1.
    let first_called = obj.borrow().get(first).is_some();
    let second_called = obj.borrow().get(second).is_some();
    if let Some(result) = try_method(obj, first)? {
        return Ok(result);
    }
    if let Some(result) = try_method(obj, second)? {
        return Ok(result);
    }
    // Both methods were called and returned non-primitive (object) values —
    // per ES spec, ToPrimitive must throw TypeError.
    if first_called && second_called {
        let (err, _) = crate::value::create_js_error_with_type(
            "Cannot convert object to primitive value",
            "TypeError",
        );
        crate::value::set_thrown_value(err);
        return Err(crate::value::JsError("TypeError".to_string()));
    }

    Ok(Value::String(to_js_string(&Value::Object(
        std::rc::Rc::new(std::cell::RefCell::new(crate::value::object::Object::new(
            crate::value::kind::ObjectKind::Ordinary,
        ))),
    ))))
}

fn resolve_hint(hint: Option<&str>) -> PrimitiveHint {
    match hint {
        Some("string") => PrimitiveHint::String,
        Some("number") => PrimitiveHint::Number,
        _ => PrimitiveHint::Default,
    }
}

fn try_to_primitive_symbol(
    obj: &Rc<RefCell<crate::value::object::Object>>,
    hint: PrimitiveHint,
) -> Result<Option<Value>, JsError> {
    let Some(to_prim_symbol) = crate::builtins::symbol::get_well_known_symbol_no_ctx("toPrimitive")
    else {
        return Ok(None);
    };
    let Value::Symbol(symbol_key) = to_prim_symbol else {
        return Ok(None);
    };
    // Use ordinary member access so an accessor is invoked (and any abrupt
    // completion propagates) rather than returning the getter function itself.
    let to_prim_method = crate::eval::member::eval_object_member(
        obj,
        symbol_key.desc.as_deref().unwrap_or(""),
        None,
    )?;
    if matches!(to_prim_method, Value::Undefined) {
        return Ok(None);
    }
    let hint_str = match hint {
        PrimitiveHint::Default => "default",
        PrimitiveHint::Number => "number",
        PrimitiveHint::String => "string",
    };
    let arg = Value::String(hint_str.to_string());
    let this_val = Value::Object(Rc::clone(obj));
    // Per ES spec §7.1.1, @@toPrimitive throwing propagates the throw.
    let result = crate::eval::call_value_with_this(to_prim_method.clone(), vec![arg], this_val)?;
    if !matches!(result, Value::Object(_)) {
        return Ok(Some(result));
    }
    let (_, js_err) = crate::value::error::create_js_error_with_type(
        "Cannot convert object to primitive value",
        "TypeError",
    );
    Err(js_err)
}

fn try_method(
    obj: &Rc<RefCell<crate::value::object::Object>>,
    method_name: &str,
) -> Result<Option<Value>, JsError> {
    let method = obj.borrow().get(method_name);
    let Some(method) = method else {
        return Ok(None);
    };
    let this_val = Value::Object(Rc::clone(obj));
    match &method {
        Value::NativeFunction(nf) => {
            // Per ES spec: if valueOf throws, ToPrimitive must propagate that throw.
            let result = nf.call(this_val, vec![])?;
            if !matches!(result, Value::Object(_)) {
                return Ok(Some(result));
            }
            Ok(None)
        }
        Value::Function(_) => {
            let result = call_value_with_this(method.clone(), vec![], this_val)?;
            if !matches!(result, Value::Object(_)) {
                return Ok(Some(result));
            }
            Ok(None)
        }
        _ => Ok(None),
    }
}

/// ToObject per ECMAScript spec - converts primitives to boxed objects
pub fn to_object(value: &Value) -> Value {
    match value {
        Value::Undefined | Value::Null => Value::Object(Rc::new(RefCell::new(
            crate::value::object::Object::new(crate::value::kind::ObjectKind::Ordinary),
        ))),
        Value::Boolean(_b) => {
            let mut obj =
                crate::value::object::Object::new(crate::value::kind::ObjectKind::Ordinary);
            obj.exotic_kind = Some(crate::value::kind::ExoticKind::Boolean);
            Value::Object(Rc::new(RefCell::new(obj)))
        }
        Value::Number(_n) => {
            let mut obj =
                crate::value::object::Object::new(crate::value::kind::ObjectKind::Ordinary);
            obj.exotic_kind = Some(crate::value::kind::ExoticKind::Number);
            Value::Object(Rc::new(RefCell::new(obj)))
        }
        Value::String(s) => {
            let mut obj =
                crate::value::object::Object::new(crate::value::kind::ObjectKind::Ordinary);
            obj.exotic_kind = Some(crate::value::kind::ExoticKind::String);
            obj.properties
                .insert("0".to_string(), Value::String(s.clone()));
            obj.elements = vec![Value::String(s.clone())];
            obj.properties
                .insert("length".to_string(), Value::Number(s.len() as f64));
            Value::Object(Rc::new(RefCell::new(obj)))
        }
        Value::Object(_) => value.clone(),
        Value::Function(_)
        | Value::NativeFunction(_)
        | Value::NativeConstructor(_)
        | Value::Class(_) => value.clone(),
        Value::Symbol(_s) => {
            let obj = crate::value::object::Object::new(crate::value::kind::ObjectKind::Ordinary);
            Value::Object(Rc::new(RefCell::new(obj)))
        }
        Value::BigInt(_) => {
            let mut obj =
                crate::value::object::Object::new(crate::value::kind::ObjectKind::Ordinary);
            obj.exotic_kind = Some(crate::value::kind::ExoticKind::BigInt);
            obj.properties.insert("_value".to_string(), value.clone());
            Value::Object(Rc::new(RefCell::new(obj)))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_same_value_nan() {
        let nan = Value::Number(f64::NAN);
        assert!(same_value(&nan, &nan), "SameValue(NaN, NaN) must be true");
        assert!(
            !same_value(&nan, &Value::Number(0.0)),
            "SameValue(NaN, 0) must be false"
        );
    }

    #[test]
    fn test_same_value_zero_sign() {
        let pos_zero = Value::Number(0.0);
        let neg_zero = Value::Number(-0.0);
        assert!(
            !same_value(&pos_zero, &neg_zero),
            "SameValue(+0, -0) must be false"
        );
        assert!(
            !same_value(&neg_zero, &pos_zero),
            "SameValue(-0, +0) must be false"
        );
    }

    #[test]
    fn test_to_uint32() {
        assert_eq!(to_uint32(-1.0), 4294967295);
        assert_eq!(to_uint32(0.0), 0);
        assert_eq!(to_uint32(0.5), 0);
        assert_eq!(to_uint32(1.0), 1);
        assert_eq!(to_uint32(4294967295.0), 4294967295);
        assert_eq!(to_uint32(4294967296.0), 0);
        assert_eq!(to_uint32(f64::NAN), 0);
        assert_eq!(to_uint32(f64::INFINITY), 0);
    }

    fn eval_bool(src: &str) -> bool {
        let mut ctx = crate::Context::new().unwrap();
        match ctx.eval(src).unwrap() {
            Value::Boolean(b) => b,
            other => panic!("expected boolean from {:?}, got {:?}", src, other),
        }
    }

    #[test]
    fn test_function_identity() {
        // Distinct functions declared in the same scope must not compare ===
        assert!(!eval_bool("function f(){}; function g(){}; f === g"));
        assert!(eval_bool("function f(){}; function g(){}; f !== g"));
        assert!(eval_bool("function f(){}; f === f"));
        // Constructor property must still point back at the same function
        assert!(eval_bool("function f(){}; f.prototype.constructor === f"));
    }

    #[test]
    fn test_class_identity() {
        assert!(eval_bool("class C {}; C === C"));
        assert!(eval_bool("class C {}; class D {}; C !== D"));
    }

    #[test]
    fn test_hex_string_to_number() {
        // Per ES §7.1.4.1: ToNumber handles 0x... hex literals.
        assert!(eval_bool("255 == '0xff'"));
        assert!(eval_bool("255 == '0XFF'"));
        assert!(eval_bool("2 == '0b10'"));
        assert!(eval_bool("15 == '0o17'"));
    }

    #[test]
    fn test_to_js_string_negative_zero() {
        // Per ECMA-262, both +0 and -0 stringify to "0".
        assert_eq!(to_js_string(&Value::Number(0.0)), "0");
        assert_eq!(to_js_string(&Value::Number(-0.0)), "0");
        // And parseInt(-0) must yield +0 (sameValue 0).
        assert!(eval_bool("parseInt(-0) === 0"));
        assert!(eval_bool("Object.is(parseInt(-0), 0)"));
    }

    #[test]
    fn test_to_uint32_edge_cases() {
        assert_eq!(super::to_uint32(-2147483649.1), 2147483647);
        assert_eq!(super::to_uint32(2147483648.0), 2147483648);
        assert_eq!(super::to_uint32(-1.0), 4294967295);
        assert_eq!(super::to_uint32(0.0), 0);
    }
}
