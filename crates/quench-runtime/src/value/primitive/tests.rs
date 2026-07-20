//! Unit tests for primitive.rs — to_primitive and to_object via the public API.

use std::cell::RefCell;
use std::rc::Rc;

use crate::value::convert::{to_object, to_primitive, PrimitiveHint};
use crate::value::kind::ObjectKind;
use crate::value::object::Object;
use crate::value::{NativeFunction, Value};

fn ctx() -> crate::Context {
    crate::Context::new().unwrap()
}

fn eval_val(src: &str) -> Value {
    ctx().eval(src).unwrap()
}

// ── primitive_direct — already-primitive values ────────────────────────────────

#[test]
fn test_to_primitive_undefined() {
    assert_eq!(
        to_primitive(&Value::Undefined, None).unwrap(),
        Value::Undefined
    );
}

#[test]
fn test_to_primitive_null() {
    assert_eq!(to_primitive(&Value::Null, None).unwrap(), Value::Null);
}

#[test]
fn test_to_primitive_boolean() {
    assert_eq!(
        to_primitive(&Value::Boolean(true), None).unwrap(),
        Value::Boolean(true)
    );
    assert_eq!(
        to_primitive(&Value::Boolean(false), None).unwrap(),
        Value::Boolean(false)
    );
}

#[test]
fn test_to_primitive_number() {
    assert_eq!(
        to_primitive(&Value::Number(42.0), None).unwrap(),
        Value::Number(42.0)
    );
}

#[test]
fn test_to_primitive_string() {
    assert_eq!(
        to_primitive(&Value::String("hello".into()), None).unwrap(),
        Value::String("hello".into())
    );
}

#[test]
fn test_to_primitive_bigint() {
    let bi = num_bigint::BigInt::from(99);
    let result = to_primitive(&Value::BigInt(Rc::new(bi)), None).unwrap();
    assert!(matches!(result, Value::BigInt(_)));
}

#[test]
fn test_to_primitive_symbol() {
    let sym = crate::value::Symbol {
        desc: Some("sym".into()),
        global: false,
    };
    let result = to_primitive(&Value::Symbol(Rc::new(sym)), None).unwrap();
    assert!(matches!(result, Value::Symbol(_)));
}

// ── NativeFunction / Class → "[Function]" ───────────────────────────────────

#[test]
fn test_to_primitive_native_function() {
    let nf = Value::NativeFunction(Rc::new(NativeFunction::new(|_| Ok(Value::Undefined))));
    let result = to_primitive(&nf, None).unwrap();
    assert_eq!(result, Value::String("[Function]".to_string()));
}

#[test]
fn test_to_primitive_native_function_hint_number() {
    let nf = Value::NativeFunction(Rc::new(NativeFunction::new(|_| Ok(Value::Undefined))));
    let result = to_primitive(&nf, Some("number")).unwrap();
    assert_eq!(result, Value::String("[Function]".to_string()));
}

#[test]
fn test_to_primitive_native_function_hint_string() {
    let nf = Value::NativeFunction(Rc::new(NativeFunction::new(|_| Ok(Value::Undefined))));
    let result = to_primitive(&nf, Some("string")).unwrap();
    assert_eq!(result, Value::String("[Function]".to_string()));
}

// ── to_primitive_object — plain object ─────────────────────────────────────

#[test]
fn test_to_primitive_object_no_methods() {
    // Plain object with no valueOf/toString returns "[object Object]"
    let obj = Value::Object(Rc::new(RefCell::new(Object::new(ObjectKind::Ordinary))));
    let result = to_primitive(&obj, None).unwrap();
    assert_eq!(result, Value::String("[object Object]".to_string()));
}

#[test]
fn test_to_primitive_object_value_of_returns_primitive() {
    let result = eval_val("var o = { valueOf() { return 42 } }; o");
    let prim = to_primitive(&result, Some("number")).unwrap();
    assert_eq!(prim, Value::Number(42.0));
}

#[test]
fn test_to_primitive_object_to_string_returns_primitive() {
    let result = eval_val("var o = { toString() { return 'custom' } }; o");
    let prim = to_primitive(&result, Some("string")).unwrap();
    assert_eq!(prim, Value::String("custom".to_string()));
}

#[test]
fn test_to_primitive_object_hint_number_prefers_value_of() {
    // Number hint: valueOf first, toString second
    let result = eval_val("var o = { valueOf() { return 1 }, toString() { return 'a' } }; o");
    let prim = to_primitive(&result, Some("number")).unwrap();
    assert_eq!(prim, Value::Number(1.0));
}

#[test]
fn test_to_primitive_object_hint_string_prefers_to_string() {
    // String hint: toString first, valueOf second
    let result = eval_val("var o = { valueOf() { return 1 }, toString() { return 'a' } }; o");
    let prim = to_primitive(&result, Some("string")).unwrap();
    assert_eq!(prim, Value::String("a".to_string()));
}

#[test]
fn test_to_primitive_object_both_return_object_throws() {
    // Both valueOf and toString return objects → TypeError
    let result = eval_val("var o = { valueOf() { return {} }, toString() { return {} } }; o");
    let prim = to_primitive(&result, Some("number"));
    assert!(prim.is_err());
}

#[test]
fn test_to_primitive_object_value_of_returns_object_to_string_works() {
    // valueOf returns object (ignored), toString returns string
    let result = eval_val("var o = { valueOf() { return {} }, toString() { return 'ok' } }; o");
    let prim = to_primitive(&result, Some("number")).unwrap();
    assert_eq!(prim, Value::String("ok".to_string()));
}

// ── Symbol.toPrimitive ────────────────────────────────────────────────────────

#[test]
fn test_to_primitive_object_symbol_to_primitive_number() {
    // Object with Symbol.toPrimitive returning number
    let result = eval_val("var o = { [Symbol.toPrimitive](hint) { return 123; } }; o");
    let prim = to_primitive(&result, Some("number")).unwrap();
    assert_eq!(prim, Value::Number(123.0));
}

#[test]
fn test_to_primitive_object_symbol_to_primitive_string() {
    // Object with Symbol.toPrimitive returning string
    let result = eval_val("var o = { [Symbol.toPrimitive](hint) { return 'symResult'; } }; o");
    let prim = to_primitive(&result, Some("string")).unwrap();
    assert_eq!(prim, Value::String("symResult".to_string()));
}

#[test]
fn test_to_primitive_object_symbol_to_primitive_default() {
    // Object with Symbol.toPrimitive, default hint
    let result = eval_val("var o = { [Symbol.toPrimitive](hint) { return 'default'; } }; o");
    let prim = to_primitive(&result, None).unwrap();
    assert_eq!(prim, Value::String("default".to_string()));
}

#[test]
fn test_to_primitive_object_symbol_to_primitive_returns_object_throws() {
    // Symbol.toPrimitive returns object → TypeError
    let result = eval_val("var o = { [Symbol.toPrimitive](hint) { return {}; } }; o");
    let prim = to_primitive(&result, Some("number"));
    assert!(prim.is_err());
}

// ── to_primitive_function — ValueFunction ─────────────────────────────────────

#[test]
fn test_to_primitive_function_default_hint() {
    // JS function: valueOf first (returns this → object), then toString
    let result = eval_val("function f() {}; f");
    let prim = to_primitive(&result, None).unwrap();
    // valueOf returns function itself (object), toString returns "[object Function]"
    assert_eq!(prim, Value::String("[Function]".to_string()));
}

#[test]
fn test_to_primitive_function_string_hint() {
    // String hint: toString first
    let result = eval_val("function f() {}; f");
    let prim = to_primitive(&result, Some("string")).unwrap();
    assert_eq!(prim, Value::String("[Function]".to_string()));
}

#[test]
fn test_to_primitive_function_with_custom_value_of() {
    // Function with custom valueOf returns 42
    let result = eval_val("function f() {}; f.valueOf = function() { return 42; }; f");
    let prim = to_primitive(&result, Some("number")).unwrap();
    assert_eq!(prim, Value::Number(42.0));
}

// ── to_object ───────────────────────────────────────────────────────────────

#[test]
fn test_to_object_undefined_returns_ordinary_object() {
    let r = to_object(&Value::Undefined);
    assert!(matches!(r, Value::Object(_)));
}

#[test]
fn test_to_object_null_returns_ordinary_object() {
    let r = to_object(&Value::Null);
    assert!(matches!(r, Value::Object(_)));
}

#[test]
fn test_to_object_bigint_sets_value_property() {
    let bi = num_bigint::BigInt::from(55);
    let r = to_object(&Value::BigInt(Rc::new(bi)));
    let obj = match r {
        Value::Object(o) => o,
        _ => panic!("expected Object"),
    };
    assert!(obj.borrow().get("_value").is_some());
}

#[test]
fn test_to_object_symbol_returns_ordinary_object() {
    let sym = crate::value::Symbol {
        desc: Some("x".into()),
        global: false,
    };
    let r = to_object(&Value::Symbol(Rc::new(sym)));
    assert!(matches!(r, Value::Object(_)));
}

// ── PrimitiveHint ───────────────────────────────────────────────────────────

#[test]
fn test_primitive_hint_eq() {
    assert_eq!(PrimitiveHint::Default, PrimitiveHint::Default);
    assert_eq!(PrimitiveHint::Number, PrimitiveHint::Number);
    assert_eq!(PrimitiveHint::String, PrimitiveHint::String);
}

#[test]
fn test_primitive_hint_ne() {
    assert_ne!(PrimitiveHint::Default, PrimitiveHint::Number);
    assert_ne!(PrimitiveHint::Number, PrimitiveHint::String);
    assert_ne!(PrimitiveHint::Default, PrimitiveHint::String);
}
