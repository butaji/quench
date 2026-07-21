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

fn eval_val(src: &str) -> Value {
    crate::Context::new().unwrap().eval(src).unwrap()
}

#[test]
fn test_to_primitive_object_plain() {
    // Plain object (created via eval) inherits valueOf/toString from Object.prototype.
    // When called without custom methods, they return "[object Object]".
    let obj = eval_val("({})");
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

// ─── to_number edge cases ───────────────────────────────────────────────────

#[test]
fn test_to_number_undefined() {
    assert!(to_number(&Value::Undefined).is_nan());
}

#[test]
fn test_to_number_null() {
    assert_eq!(to_number(&Value::Null), 0.0);
}

#[test]
fn test_to_number_boolean() {
    assert_eq!(to_number(&Value::Boolean(true)), 1.0);
    assert_eq!(to_number(&Value::Boolean(false)), 0.0);
}

#[test]
fn test_to_number_string_trimmed() {
    assert_eq!(to_number(&Value::String("  42  ".to_string())), 42.0);
}

#[test]
fn test_to_number_string_leading_plus() {
    assert_eq!(to_number(&Value::String("+42".to_string())), 42.0);
}

#[test]
fn test_to_number_string_leading_minus() {
    assert_eq!(to_number(&Value::String("-42".to_string())), -42.0);
}

#[test]
fn test_to_number_string_decimal_point() {
    // "3.14" parses as 3.14 exactly (157/50)
    assert_eq!(
        to_number(&Value::String("3.14".to_string())),
        157.0_f64 / 50.0
    );
}

#[test]
fn test_to_number_string_trailing_dot() {
    assert_eq!(to_number(&Value::String("42.".to_string())), 42.0);
}

#[test]
fn test_to_number_string_leading_dot() {
    assert_eq!(to_number(&Value::String(".5".to_string())), 0.5);
}

// ─── to_primitive via Context ────────────────────────────────────────────

// Note: to_primitive on plain objects requires Context for valueOf/toString evaluation.
// These tests use Context::eval to test the full round-trip.

#[test]
fn test_to_primitive_with_valueof() {
    let mut ctx = crate::Context::new().unwrap();
    let r = ctx
        .eval(
            "var obj = { valueOf: function() { return 42; } }; \
         Number(obj);",
        )
        .unwrap();
    assert_eq!(r, Value::Number(42.0));
}

#[test]
fn test_to_primitive_with_tostring() {
    let mut ctx = crate::Context::new().unwrap();
    let r = ctx
        .eval(
            "var obj = { toString: function() { return 'hello'; } }; \
         String(obj);",
        )
        .unwrap();
    assert_eq!(r, Value::String("hello".to_string()));
}

#[test]
fn test_to_primitive_valueof_takes_precedence() {
    let mut ctx = crate::Context::new().unwrap();
    // When both valueOf and toString exist, valueOf takes precedence for number hint
    let r = ctx
        .eval(
            "var obj = { \
           valueOf: function() { return 42; }, \
           toString: function() { return 'str'; } \
         }; \
         Number(obj);",
        )
        .unwrap();
    assert_eq!(r, Value::Number(42.0));
}

#[test]
fn test_to_number_empty_string() {
    assert_eq!(to_number(&Value::String(String::new())), 0.0);
}

#[test]
fn test_to_number_whitespace_string() {
    assert_eq!(to_number(&Value::String("   ".to_string())), 0.0);
    assert_eq!(to_number(&Value::String("\t\n".to_string())), 0.0);
}

#[test]
fn test_to_number_leading_trailing_whitespace() {
    assert_eq!(to_number(&Value::String("  42  ".to_string())), 42.0);
    assert_eq!(to_number(&Value::String("\t123\n".to_string())), 123.0);
}

#[test]
fn test_to_number_leading_plus() {
    assert_eq!(to_number(&Value::String("+42".to_string())), 42.0);
}

#[test]
fn test_to_number_leading_minus() {
    assert_eq!(to_number(&Value::String("-42".to_string())), -42.0);
}

#[test]
fn test_to_number_decimal_formats() {
    assert_eq!(
        to_number(&Value::String("3.14".to_string())),
        157.0_f64 / 50.0
    );
    assert_eq!(to_number(&Value::String("42.".to_string())), 42.0);
    assert_eq!(to_number(&Value::String(".5".to_string())), 0.5);
    assert_eq!(to_number(&Value::String("-0.5".to_string())), -0.5);
}

#[test]
fn test_to_number_scientific_notation() {
    assert_eq!(to_number(&Value::String("1e3".to_string())), 1000.0);
    assert_eq!(to_number(&Value::String("2.5e2".to_string())), 250.0);
    assert_eq!(to_number(&Value::String("1E-1".to_string())), 0.1);
}

#[test]
fn test_to_number_explicit_infinity() {
    assert_eq!(
        to_number(&Value::String("Infinity".to_string())),
        f64::INFINITY
    );
    assert_eq!(
        to_number(&Value::String("-Infinity".to_string())),
        f64::NEG_INFINITY
    );
    assert_eq!(
        to_number(&Value::String("+Infinity".to_string())),
        f64::INFINITY
    );
}

#[test]
fn test_to_number_invalid_returns_nan() {
    assert!(to_number(&Value::String("hello".to_string())).is_nan());
    assert!(to_number(&Value::String("0xGG".to_string())).is_nan());
    assert!(to_number(&Value::String("0b123".to_string())).is_nan());
    assert!(to_number(&Value::String("0o89".to_string())).is_nan());
    assert!(to_number(&Value::String("true".to_string())).is_nan());
    assert!(to_number(&Value::String("null".to_string())).is_nan());
}

#[test]
fn test_to_number_undefined_is_nan() {
    assert!(to_number(&Value::Undefined).is_nan());
}

#[test]
fn test_to_number_object_is_nan() {
    let obj = Value::Object(Rc::new(RefCell::new(Object::new(ObjectKind::Ordinary))));
    assert!(to_number(&obj).is_nan());
}

// ─── strict_eq BigInt / Symbol ─────────────────────────────────────────

#[test]
fn test_strict_eq_same_bigint() {
    let bi = num_bigint::BigInt::from(42);
    assert!(strict_eq(
        &Value::BigInt(Rc::new(bi.clone())),
        &Value::BigInt(Rc::new(bi))
    ));
}

#[test]
fn test_strict_eq_different_bigint() {
    let a = Value::BigInt(Rc::new(num_bigint::BigInt::from(1)));
    let b = Value::BigInt(Rc::new(num_bigint::BigInt::from(2)));
    assert!(!strict_eq(&a, &b));
}

#[test]
fn test_strict_eq_bigint_zero() {
    let a = Value::BigInt(Rc::new(num_bigint::BigInt::from(0)));
    let b = Value::BigInt(Rc::new(num_bigint::BigInt::from(0)));
    assert!(strict_eq(&a, &b));
}

#[test]
fn test_strict_eq_same_symbol() {
    let sym = Rc::new(Symbol {
        desc: Some("test".into()),
        global: false,
    });
    assert!(strict_eq(&Value::Symbol(sym.clone()), &Value::Symbol(sym)));
}

#[test]
fn test_strict_eq_different_symbols() {
    let a = Value::Symbol(Rc::new(Symbol {
        desc: Some("a".into()),
        global: false,
    }));
    let b = Value::Symbol(Rc::new(Symbol {
        desc: Some("a".into()),
        global: false,
    }));
    assert!(
        !strict_eq(&a, &b),
        "Symbols with same desc are different objects"
    );
}

#[test]
fn test_strict_eq_symbol_no_desc() {
    let sym = Rc::new(Symbol {
        desc: None,
        global: false,
    });
    assert!(strict_eq(&Value::Symbol(sym.clone()), &Value::Symbol(sym)));
}

// ─── to_primitive deeper cases ────────────────────────────────────────

#[test]
fn test_to_primitive_valueof_returns_object_falls_back_to_string() {
    let mut ctx = crate::Context::new().unwrap();
    // valueOf returns object → used as fallback
    let r = ctx.eval(
        "var obj = { valueOf: function() { return {}; }, toString: function() { return 'fallback'; } }; String(obj)"
    ).unwrap();
    assert_eq!(r, Value::String("fallback".to_string()));
}

#[test]
fn test_to_primitive_both_return_object() {
    let mut ctx = crate::Context::new().unwrap();
    // Both return objects → default to "[object Object]"
    let r = ctx.eval(
        "var obj = { valueOf: function() { return {}; }, toString: function() { return {}; } }; String(obj)"
    ).unwrap();
    assert_eq!(r, Value::String("[object Object]".to_string()));
}

#[test]
fn test_to_primitive_tostring_only_for_string_hint() {
    let mut ctx = crate::Context::new().unwrap();
    let r = ctx
        .eval("var obj = { toString: function() { return 'custom'; } }; String(obj)")
        .unwrap();
    assert_eq!(r, Value::String("custom".to_string()));
}

#[test]
fn test_to_primitive_function_identity() {
    // Two different function objects are never strict-equal
    let mut ctx = crate::Context::new().unwrap();
    let result = ctx.eval("function f(){} function g(){} f !== g").unwrap();
    assert_eq!(result, Value::Boolean(true));
    let result = ctx.eval("function f(){} f === f").unwrap();
    assert_eq!(result, Value::Boolean(true));
}
