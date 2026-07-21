//! Tests for value conversion operations (to_primitive, to_number, etc.)
//!
//! These are NOT test262 replicas — they verify the behavior of the Rust
//! runtime's spec-op layer directly.

use std::cell::RefCell;
use std::rc::Rc;

use crate::value::convert::{
    loose_eq, same_value, strict_eq, to_bool, to_js_string, to_number, to_number_unchecked,
    to_object, to_primitive, to_uint32, try_to_number, PrimitiveHint,
};
use crate::value::kind::ObjectKind;
use crate::value::object::Object;
use crate::value::NativeFunction;
use crate::value::Symbol;
use crate::value::Value;

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
    assert_eq!(to_uint32(-2147483649.1), 2147483647);
    assert_eq!(to_uint32(2147483648.0), 2147483648);
    assert_eq!(to_uint32(-1.0), 4294967295);
    assert_eq!(to_uint32(0.0), 0);
}

// ─── to_bool ────────────────────────────────────────────────────────────────

#[test]
fn test_to_bool_falsy_values() {
    assert!(!to_bool(&Value::Undefined));
    assert!(!to_bool(&Value::Null));
    assert!(!to_bool(&Value::Boolean(false)));
    assert!(!to_bool(&Value::Number(0.0)));
    assert!(!to_bool(&Value::Number(-0.0)));
    assert!(!to_bool(&Value::Number(f64::NAN)));
    assert!(!to_bool(&Value::String(String::new())));
}

#[test]
fn test_to_bool_truthy_values() {
    assert!(to_bool(&Value::Boolean(true)));
    assert!(to_bool(&Value::Number(1.0)));
    assert!(to_bool(&Value::Number(f64::INFINITY)));
    assert!(to_bool(&Value::Number(-f64::INFINITY)));
    assert!(to_bool(&Value::String("0".to_string())));
    assert!(to_bool(&Value::String("false".to_string())));
}

#[test]
fn test_to_bool_objects_are_truthy() {
    let obj = Value::Object(Rc::new(RefCell::new(Object::new(ObjectKind::Ordinary))));
    assert!(to_bool(&obj), "All objects are truthy in JS");
}

#[test]
fn test_to_bool_bigint_zero_is_falsy() {
    let bi = num_bigint::BigInt::from(0);
    assert!(!to_bool(&Value::BigInt(Rc::new(bi))), "BigInt(0) is falsy");
}

#[test]
fn test_to_bool_bigint_nonzero_is_truthy() {
    let bi = num_bigint::BigInt::from(1);
    assert!(to_bool(&Value::BigInt(Rc::new(bi))), "BigInt(1) is truthy");
}

// ─── to_number ──────────────────────────────────────────────────────────────

#[test]
fn test_to_number_primitives() {
    assert!(to_number(&Value::Undefined).is_nan());
    assert_eq!(to_number(&Value::Null), 0.0);
    assert_eq!(to_number(&Value::Boolean(false)), 0.0);
    assert_eq!(to_number(&Value::Boolean(true)), 1.0);
}

#[test]
fn test_to_number_strings() {
    assert_eq!(to_number(&Value::String("42".to_string())), 42.0);
    assert_eq!(to_number(&Value::String("".to_string())), 0.0);
    assert_eq!(to_number(&Value::String("  ".to_string())), 0.0);
    assert!(to_number(&Value::String("hello".to_string())).is_nan());
    assert_eq!(to_number(&Value::String("1.5".to_string())), 1.5);
    assert_eq!(to_number(&Value::String("1e3".to_string())), 1000.0);
}

#[test]
fn test_to_number_object_calls_to_primitive() {
    // Object coerces to number via valueOf/toString
    let obj = Value::Object(Rc::new(RefCell::new(Object::new(ObjectKind::Ordinary))));
    // Plain object valueOf returns the object itself → NaN
    assert!(to_number(&obj).is_nan());
}

// ─── try_to_number ──────────────────────────────────────────────────────────

#[test]
fn test_try_to_number_valid() {
    assert_eq!(try_to_number(&Value::Number(42.0)).unwrap(), 42.0);
    assert_eq!(
        try_to_number(&Value::String("123".to_string())).unwrap(),
        123.0
    );
}

#[test]
fn test_try_to_number_symbol_throws() {
    let sym = Value::Symbol(Rc::new(Symbol {
        desc: Some("test".into()),
        global: false,
    }));
    assert!(
        try_to_number(&sym).is_err(),
        "ToNumber(Symbol) must throw TypeError"
    );
}

// ─── to_number_unchecked ────────────────────────────────────────────────────

#[test]
fn test_to_number_unchecked_primitives() {
    assert_eq!(to_number_unchecked(&Value::Number(3.0)), 3.0);
    assert_eq!(to_number_unchecked(&Value::Boolean(true)), 1.0);
    assert_eq!(to_number_unchecked(&Value::Boolean(false)), 0.0);
    assert_eq!(to_number_unchecked(&Value::Null), 0.0);
}

// ─── strict_eq ─────────────────────────────────────────────────────────────

#[test]
fn test_strict_eq_primitives() {
    assert!(strict_eq(&Value::Number(1.0), &Value::Number(1.0)));
    assert!(!strict_eq(&Value::Number(1.0), &Value::Number(2.0)));
    assert!(strict_eq(
        &Value::String("a".to_string()),
        &Value::String("a".to_string())
    ));
    assert!(!strict_eq(
        &Value::String("a".to_string()),
        &Value::String("b".to_string())
    ));
    assert!(strict_eq(&Value::Boolean(true), &Value::Boolean(true)));
    assert!(!strict_eq(&Value::Boolean(true), &Value::Boolean(false)));
}

#[test]
fn test_strict_eq_nan_is_false() {
    let nan = Value::Number(f64::NAN);
    assert!(!strict_eq(&nan, &nan), "NaN !== NaN in strict equality");
    assert!(!strict_eq(&nan, &Value::Number(0.0)));
}

#[test]
fn test_strict_eq_null_undefined() {
    assert!(strict_eq(&Value::Undefined, &Value::Undefined));
    assert!(strict_eq(&Value::Null, &Value::Null));
    assert!(!strict_eq(&Value::Null, &Value::Undefined));
}

#[test]
fn test_strict_eq_different_types() {
    assert!(!strict_eq(&Value::Number(0.0), &Value::Null));
    assert!(!strict_eq(
        &Value::String("0".to_string()),
        &Value::Number(0.0)
    ));
    assert!(!strict_eq(&Value::Boolean(false), &Value::Number(0.0)));
}

#[test]
fn test_strict_eq_objects() {
    let obj1 = Value::Object(Rc::new(RefCell::new(Object::new(ObjectKind::Ordinary))));
    let obj2 = Value::Object(Rc::new(RefCell::new(Object::new(ObjectKind::Ordinary))));
    // Different objects are never strict equal
    assert!(!strict_eq(&obj1, &obj2));
    // Same object reference is strict equal
    assert!(strict_eq(&obj1, &obj1));
}

// ─── loose_eq (==) ──────────────────────────────────────────────────────────

#[test]
fn test_loose_eq_null_undefined() {
    assert!(loose_eq(&Value::Null, &Value::Undefined));
    assert!(loose_eq(&Value::Undefined, &Value::Null));
}

#[test]
fn test_loose_eq_number_string() {
    assert!(loose_eq(
        &Value::Number(42.0),
        &Value::String("42".to_string())
    ));
    assert!(loose_eq(
        &Value::String("42".to_string()),
        &Value::Number(42.0)
    ));
    assert!(!loose_eq(
        &Value::Number(42.0),
        &Value::String("43".to_string())
    ));
}

#[test]
fn test_loose_eq_boolean_coercion() {
    assert!(loose_eq(&Value::Boolean(true), &Value::Number(1.0)));
    assert!(loose_eq(&Value::Boolean(false), &Value::Number(0.0)));
    assert!(loose_eq(
        &Value::Boolean(false),
        &Value::String("".to_string())
    ));
}

#[test]
fn test_loose_eq_same_type() {
    // Same type falls back to strict_eq
    assert!(loose_eq(&Value::Number(1.0), &Value::Number(1.0)));
    assert!(!loose_eq(&Value::Number(1.0), &Value::Number(2.0)));
}

// ─── to_primitive ────────────────────────────────────────────────────────────

#[test]
fn test_to_primitive_already_primitive() {
    assert_eq!(
        to_primitive(&Value::Number(42.0), None).unwrap(),
        Value::Number(42.0)
    );
    assert_eq!(
        to_primitive(&Value::String("hi".to_string()), None).unwrap(),
        Value::String("hi".to_string())
    );
    assert_eq!(
        to_primitive(&Value::Boolean(true), None).unwrap(),
        Value::Boolean(true)
    );
    assert_eq!(
        to_primitive(&Value::Undefined, None).unwrap(),
        Value::Undefined
    );
    assert_eq!(to_primitive(&Value::Null, None).unwrap(), Value::Null);
}

#[test]
fn test_to_primitive_function_hint_string() {
    // Function's toString returns "[object Function]" which is a string
    let nf = Value::NativeFunction(Rc::new(NativeFunction::new(|_| Ok(Value::Undefined))));
    let result = to_primitive(&nf, Some("string")).unwrap();
    assert!(matches!(result, Value::String(_)));
}

#[test]
fn test_to_primitive_object_plain() {
    // Plain object with no custom valueOf/toString falls back to "[object Object]"
    let obj = Value::Object(Rc::new(RefCell::new(Object::new(ObjectKind::Ordinary))));
    let result = to_primitive(&obj, None).unwrap();
    assert_eq!(result, Value::String("[object Object]".to_string()));
}

// ─── to_object ─────────────────────────────────────────────────────────────

#[test]
fn test_to_object_null_undefined() {
    // Per spec, ToObject(null/undefined) returns a new ordinary object (not a real error)
    let r = to_object(&Value::Null);
    assert!(matches!(r, Value::Object(_)));
    let r2 = to_object(&Value::Undefined);
    assert!(matches!(r2, Value::Object(_)));
}

#[test]
fn test_to_object_boolean() {
    let r = to_object(&Value::Boolean(true));
    assert!(matches!(r, Value::Object(_)));
    let obj = match r {
        Value::Object(o) => o,
        _ => unreachable!(),
    };
    assert!(matches!(
        obj.borrow().exotic_kind,
        Some(crate::value::kind::ExoticKind::Boolean)
    ));
}

#[test]
fn test_to_object_number() {
    let r = to_object(&Value::Number(42.0));
    assert!(matches!(r, Value::Object(_)));
    let obj = match r {
        Value::Object(o) => o,
        _ => unreachable!(),
    };
    assert!(matches!(
        obj.borrow().exotic_kind,
        Some(crate::value::kind::ExoticKind::Number)
    ));
}

#[test]
fn test_to_object_string() {
    let r = to_object(&Value::String("abc".to_string()));
    assert!(matches!(r, Value::Object(_)));
    let obj = match r {
        Value::Object(o) => o,
        _ => unreachable!(),
    };
    assert!(matches!(
        obj.borrow().exotic_kind,
        Some(crate::value::kind::ExoticKind::String)
    ));
    // String object stores the full string at key "0" and has a length property
    assert_eq!(
        obj.borrow().get("0"),
        Some(Value::String("abc".to_string()))
    );
    assert_eq!(obj.borrow().get("length"), Some(Value::Number(3.0)));
}

#[test]
fn test_to_object_bigint() {
    let bi = num_bigint::BigInt::from(123);
    let r = to_object(&Value::BigInt(Rc::new(bi)));
    assert!(matches!(r, Value::Object(_)));
    let obj = match r {
        Value::Object(o) => o,
        _ => unreachable!(),
    };
    assert!(matches!(
        obj.borrow().exotic_kind,
        Some(crate::value::kind::ExoticKind::BigInt)
    ));
    assert!(obj.borrow().get("_value").is_some());
}

#[test]
fn test_to_object_symbol() {
    let r = to_object(&Value::Symbol(Rc::new(Symbol {
        desc: Some("sym".into()),
        global: false,
    })));
    assert!(matches!(r, Value::Object(_)));
}

#[test]
fn test_to_object_object_passthrough() {
    let obj = Value::Object(Rc::new(RefCell::new(Object::new(ObjectKind::Ordinary))));
    let obj_clone = obj.clone();
    let r = to_object(&obj);
    assert!(matches!(r, Value::Object(_)));
    // to_object on object returns same object (passthrough)
    drop(obj_clone);
}

// ─── PrimitiveHint ─────────────────────────────────────────────────────────

#[test]
fn test_primitive_hint_variants() {
    assert_eq!(PrimitiveHint::Default, PrimitiveHint::Default);
    assert_eq!(PrimitiveHint::Number, PrimitiveHint::Number);
    assert_eq!(PrimitiveHint::String, PrimitiveHint::String);
    assert_ne!(PrimitiveHint::Number, PrimitiveHint::String);
}
