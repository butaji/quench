//! Unit tests for compare.rs spec ops — strict_eq, loose_eq, same_value.

use num_bigint::BigInt;
use quench_runtime::value::compare::{
    loose_eq, parse_number_string, primitive_for_compare, same_value, same_value_numbers,
    strict_eq, to_primitive_for_compare, to_primitive_for_compare_strict,
};
use quench_runtime::value::function::{NativeConstructor, NativeFunction, ValueFunction};
use quench_runtime::value::{Object, ObjectKind, Symbol, Value};
use std::cell::RefCell;
use std::rc::Rc;

fn obj() -> Value {
    Value::Object(Rc::new(RefCell::new(Object::new(ObjectKind::Ordinary))))
}

fn sym(desc: &str) -> Value {
    Value::Symbol(Rc::new(Symbol {
        desc: Some(Rc::from(desc)),
        global: false,
    }))
}

fn big(n: i32) -> Value {
    Value::BigInt(Rc::new(BigInt::from(n)))
}

fn make_env() -> Rc<RefCell<quench_runtime::env::Environment>> {
    Rc::new(RefCell::new(quench_runtime::env::Environment::new()))
}

fn native_fn() -> Value {
    Value::NativeFunction(Rc::new(NativeFunction::new(|_| Ok(Value::Undefined))))
}

fn native_ctor() -> Value {
    let proto = Rc::new(RefCell::new(Object::new(ObjectKind::Ordinary)));
    Value::NativeConstructor(Rc::new(NativeConstructor::new(
        |_| Ok(Value::Undefined),
        proto,
    )))
}

fn func() -> Value {
    Value::Function(ValueFunction::new(
        None,
        vec![],
        vec![],
        make_env(),
        false,
        false,
    ))
}

// ── strict_eq: primitives ─────────────────────────────────────────────────

#[test]
fn strict_eq_undefined_undefined() {
    assert!(strict_eq(&Value::Undefined, &Value::Undefined));
}

#[test]
fn strict_eq_null_null() {
    assert!(strict_eq(&Value::Null, &Value::Null));
}

#[test]
fn strict_eq_boolean_same() {
    assert!(strict_eq(&Value::Boolean(true), &Value::Boolean(true)));
    assert!(strict_eq(&Value::Boolean(false), &Value::Boolean(false)));
}

#[test]
fn strict_eq_boolean_different() {
    assert!(!strict_eq(&Value::Boolean(true), &Value::Boolean(false)));
}

#[test]
fn strict_eq_number_same() {
    assert!(strict_eq(&Value::Number(0.0), &Value::Number(0.0)));
    assert!(strict_eq(&Value::Number(42.0), &Value::Number(42.0)));
    assert!(strict_eq(&Value::Number(-0.0), &Value::Number(-0.0)));
}

#[test]
fn strict_eq_number_different() {
    assert!(!strict_eq(&Value::Number(0.0), &Value::Number(1.0)));
    assert!(!strict_eq(&Value::Number(42.0), &Value::Number(42.1)));
}

#[test]
fn strict_eq_string_same() {
    assert!(strict_eq(
        &Value::String("a".into()),
        &Value::String("a".into())
    ));
    assert!(strict_eq(
        &Value::String("".into()),
        &Value::String("".into())
    ));
}

#[test]
fn strict_eq_string_different() {
    assert!(!strict_eq(
        &Value::String("a".into()),
        &Value::String("b".into())
    ));
    assert!(!strict_eq(
        &Value::String("aa".into()),
        &Value::String("a".into())
    ));
}

#[test]
fn strict_eq_bigint_same() {
    assert!(strict_eq(&big(0), &big(0)));
    assert!(strict_eq(&big(42), &big(42)));
    assert!(strict_eq(&big(-1), &big(-1)));
}

#[test]
fn strict_eq_bigint_different() {
    assert!(!strict_eq(&big(0), &big(1)));
    assert!(!strict_eq(&big(1), &big(-1)));
}

// ── strict_eq: cross-type (different discriminants) ──────────────────────

#[test]
fn strict_eq_undefined_not_null() {
    assert!(!strict_eq(&Value::Undefined, &Value::Null));
    assert!(!strict_eq(&Value::Null, &Value::Undefined));
}

#[test]
fn strict_eq_cross_type_never_true() {
    let cases = [
        (Value::Undefined, Value::Null),
        (Value::Undefined, Value::Boolean(true)),
        (Value::Undefined, Value::Number(0.0)),
        (Value::Undefined, Value::String("".into())),
        (Value::Undefined, big(0)),
        (Value::Null, Value::Boolean(false)),
        (Value::Null, Value::Number(0.0)),
        (Value::Boolean(true), Value::Number(1.0)),
        (Value::Number(0.0), Value::String("0".into())),
        (Value::Number(1.0), big(1)),
    ];
    for (a, b) in cases {
        assert!(
            !strict_eq(&a, &b),
            "strict_eq({:?}, {:?}) should be false",
            a,
            b
        );
    }
}

// ── strict_eq: symbols ──────────────────────────────────────────────────

#[test]
fn strict_eq_symbol_same_identity() {
    let s = sym("foo");
    assert!(strict_eq(&s, &s));
}

#[test]
fn strict_eq_symbol_different_identity() {
    let a = sym("foo");
    let b = sym("foo");
    assert!(!strict_eq(&a, &b), "different Symbol() objects are not ===");
}

// ── strict_eq: objects ─────────────────────────────────────────────────

#[test]
fn strict_eq_object_same_identity() {
    let o = obj();
    assert!(strict_eq(&o, &o));
}

#[test]
fn strict_eq_object_different_instances() {
    let a = obj();
    let b = obj();
    assert!(!strict_eq(&a, &b), "different Object instances are not ===");
}

// ── strict_eq: functions ───────────────────────────────────────────────

#[test]
fn strict_eq_function_same_identity() {
    let f = func();
    assert!(strict_eq(&f, &f));
}

#[test]
fn strict_eq_function_different_instances() {
    let a = func();
    let b = func();
    assert!(
        !strict_eq(&a, &b),
        "different function instances are not ==="
    );
}

#[test]
fn strict_eq_native_function_same_identity() {
    let nf = native_fn();
    assert!(strict_eq(&nf, &nf));
}

#[test]
fn strict_eq_native_function_different_instances() {
    let a = native_fn();
    let b = native_fn();
    assert!(!strict_eq(&a, &b), "different NativeFunction are not ===");
}

#[test]
fn strict_eq_native_constructor_same_identity() {
    let nc = native_ctor();
    assert!(strict_eq(&nc, &nc));
}

#[test]
fn strict_eq_native_constructor_different_instances() {
    let a = native_ctor();
    let b = native_ctor();
    assert!(!strict_eq(&a, &b));
}

// ── strict_eq: cross-category ──────────────────────────────────────────

#[test]
fn strict_eq_function_vs_object() {
    let f = func();
    let o = obj();
    assert!(!strict_eq(&f, &o));
}

#[test]
fn strict_eq_function_vs_native_function() {
    let f = func();
    let nf = native_fn();
    assert!(!strict_eq(&f, &nf));
}

#[test]
fn strict_eq_object_vs_number() {
    assert!(!strict_eq(&obj(), &Value::Number(0.0)));
}

// ── same_value: NaN ────────────────────────────────────────────────────

#[test]
fn same_value_nan_equals_nan() {
    assert!(same_value(
        &Value::Number(f64::NAN),
        &Value::Number(f64::NAN)
    ));
}

#[test]
fn same_value_numbers_nan_equals_nan() {
    assert!(same_value_numbers(f64::NAN, f64::NAN));
}

#[test]
fn same_value_numbers_positive_zero() {
    assert!(same_value_numbers(0.0, 0.0));
}

#[test]
fn same_value_numbers_negative_zero() {
    assert!(same_value_numbers(-0.0, -0.0));
}

#[test]
fn same_value_numbers_zero_different_signs() {
    assert!(!same_value_numbers(0.0, -0.0), "+0 !== -0 in SameValue");
    assert!(!same_value_numbers(-0.0, 0.0), "-0 !== +0 in SameValue");
}

#[test]
fn same_value_numbers_regular() {
    assert!(same_value_numbers(1.0, 1.0));
    assert!(same_value_numbers(-1.0, -1.0));
    assert!(!same_value_numbers(1.0, 2.0));
    assert!(same_value_numbers(f64::INFINITY, f64::INFINITY));
    assert!(!same_value_numbers(f64::NEG_INFINITY, f64::INFINITY));
}

// ── same_value: primitives ──────────────────────────────────────────────

#[test]
fn same_value_undefined() {
    assert!(same_value(&Value::Undefined, &Value::Undefined));
}

#[test]
fn same_value_null() {
    assert!(same_value(&Value::Null, &Value::Null));
}

#[test]
fn same_value_boolean() {
    assert!(same_value(&Value::Boolean(true), &Value::Boolean(true)));
    assert!(same_value(&Value::Boolean(false), &Value::Boolean(false)));
    assert!(!same_value(&Value::Boolean(true), &Value::Boolean(false)));
}

#[test]
fn same_value_string() {
    assert!(same_value(
        &Value::String("a".into()),
        &Value::String("a".into())
    ));
    assert!(!same_value(
        &Value::String("a".into()),
        &Value::String("b".into())
    ));
}

#[test]
fn same_value_bigint() {
    assert!(same_value(&big(42), &big(42)));
    assert!(!same_value(&big(42), &big(43)));
}

#[test]
fn same_value_symbol_same_identity() {
    let s = sym("x");
    assert!(same_value(&s, &s));
}

#[test]
fn same_value_symbol_different_identity() {
    let a = sym("x");
    let b = sym("x");
    assert!(
        !same_value(&a, &b),
        "different Symbol() have different SameValue"
    );
}

// ── same_value: cross-discriminant always false ─────────────────────────

#[test]
fn same_value_cross_discriminant_always_false() {
    let pairs = [
        (Value::Undefined, Value::Null),
        (Value::Null, Value::Undefined),
        (Value::Number(0.0), Value::String("0".into())),
        (Value::Boolean(true), Value::Number(1.0)),
    ];
    for (a, b) in pairs {
        assert!(
            !same_value(&a, &b),
            "same_value({:?}, {:?}) should be false",
            a,
            b
        );
    }
}

// ── same_value: functions ───────────────────────────────────────────────

#[test]
fn same_value_function_same_identity() {
    let f = func();
    assert!(same_value(&f, &f));
}

#[test]
fn same_value_function_different_instances() {
    let a = func();
    let b = func();
    assert!(!same_value(&a, &b));
}

#[test]
fn same_value_native_function() {
    let a = native_fn();
    let b = native_fn();
    assert!(!same_value(&a, &b));
}

// ── same_value: vs strict_eq on edge cases ─────────────────────────────

#[test]
fn same_value_vs_strict_eq_nan() {
    // strict_eq: NaN !== NaN (via discriminant); same_value: NaN === NaN
    assert!(!strict_eq(
        &Value::Number(f64::NAN),
        &Value::Number(f64::NAN)
    ));
    assert!(same_value(
        &Value::Number(f64::NAN),
        &Value::Number(f64::NAN)
    ));
}

#[test]
fn same_value_vs_strict_eq_zero_signs() {
    // strict_eq: +0 === -0; same_value: +0 !== -0
    assert!(strict_eq(&Value::Number(0.0), &Value::Number(-0.0)));
    assert!(!same_value(&Value::Number(0.0), &Value::Number(-0.0)));
}

// ── loose_eq: null/undefined ────────────────────────────────────────────

#[test]
fn loose_eq_null_undefined() {
    assert!(loose_eq(&Value::Null, &Value::Undefined));
    assert!(loose_eq(&Value::Undefined, &Value::Null));
}

#[test]
fn loose_eq_null_undefined_not_equal_to_others() {
    assert!(!loose_eq(&Value::Null, &Value::Boolean(false)));
    assert!(!loose_eq(&Value::Undefined, &Value::Boolean(false)));
    assert!(!loose_eq(&Value::Null, &Value::Number(0.0)));
    assert!(!loose_eq(&Value::Undefined, &Value::Number(0.0)));
    assert!(!loose_eq(&Value::Null, &Value::String("null".into())));
    assert!(!loose_eq(
        &Value::Undefined,
        &Value::String("undefined".into())
    ));
}

// ── loose_eq: primitives same type ──────────────────────────────────────

#[test]
fn loose_eq_boolean_same() {
    assert!(loose_eq(&Value::Boolean(true), &Value::Boolean(true)));
    assert!(loose_eq(&Value::Boolean(false), &Value::Boolean(false)));
    assert!(!loose_eq(&Value::Boolean(true), &Value::Boolean(false)));
}

#[test]
fn loose_eq_number_same() {
    assert!(loose_eq(&Value::Number(0.0), &Value::Number(0.0)));
    assert!(loose_eq(&Value::Number(1.0), &Value::Number(1.0)));
    assert!(!loose_eq(&Value::Number(0.0), &Value::Number(1.0)));
}

#[test]
fn loose_eq_string_same() {
    assert!(loose_eq(
        &Value::String("a".into()),
        &Value::String("a".into())
    ));
    assert!(!loose_eq(
        &Value::String("a".into()),
        &Value::String("b".into())
    ));
}

#[test]
fn loose_eq_bigint_same() {
    assert!(loose_eq(&big(0), &big(0)));
    assert!(loose_eq(&big(42), &big(42)));
    assert!(!loose_eq(&big(0), &big(1)));
}

// ── loose_eq: number-string coercion ─────────────────────────────────────

#[allow(clippy::approx_constant)]
#[test]
fn loose_eq_number_string() {
    assert!(loose_eq(&Value::Number(0.0), &Value::String("0".into())));
    assert!(loose_eq(&Value::String("0".into()), &Value::Number(0.0)));
    assert!(loose_eq(&Value::Number(42.0), &Value::String("42".into())));
    assert!(loose_eq(
        &Value::Number(3.14),
        &Value::String("3.14".into())
    ));
}

#[test]
fn loose_eq_number_string_no_match() {
    assert!(!loose_eq(&Value::Number(0.0), &Value::String("1".into())));
    assert!(!loose_eq(&Value::String("42".into()), &Value::Number(0.0)));
}

#[test]
fn loose_eq_number_empty_string() {
    assert!(loose_eq(&Value::Number(0.0), &Value::String("".into())));
    assert!(loose_eq(&Value::String("   ".into()), &Value::Number(0.0)));
}

#[test]
fn loose_eq_number_hex_string() {
    assert!(loose_eq(
        &Value::Number(255.0),
        &Value::String("0xFF".into())
    ));
    assert!(loose_eq(
        &Value::Number(255.0),
        &Value::String("0xff".into())
    ));
    assert!(loose_eq(&Value::Number(15.0), &Value::String("0xf".into())));
}

#[test]
fn loose_eq_number_binary_string() {
    assert!(loose_eq(
        &Value::Number(5.0),
        &Value::String("0b101".into())
    ));
    assert!(loose_eq(
        &Value::Number(7.0),
        &Value::String("0B111".into())
    ));
}

#[test]
fn loose_eq_number_octal_string() {
    assert!(loose_eq(&Value::Number(8.0), &Value::String("0o10".into())));
    assert!(loose_eq(
        &Value::Number(64.0),
        &Value::String("0O100".into())
    ));
}

#[test]
fn loose_eq_number_invalid_string() {
    assert!(!loose_eq(&Value::Number(1.0), &Value::String("abc".into())));
    assert!(!loose_eq(&Value::String("xyz".into()), &Value::Number(0.0)));
}

// ── loose_eq: boolean coercion ───────────────────────────────────────────

#[test]
fn loose_eq_boolean_number() {
    assert!(loose_eq(&Value::Boolean(true), &Value::Number(1.0)));
    assert!(loose_eq(&Value::Boolean(false), &Value::Number(0.0)));
    assert!(loose_eq(&Value::Number(1.0), &Value::Boolean(true)));
    assert!(loose_eq(&Value::Number(0.0), &Value::Boolean(false)));
}

#[test]
fn loose_eq_boolean_string() {
    assert!(loose_eq(&Value::Boolean(true), &Value::String("1".into())));
    assert!(loose_eq(&Value::Boolean(false), &Value::String("0".into())));
    // "true" → NaN, true → 1; NaN != 1
    assert!(!loose_eq(
        &Value::String("true".into()),
        &Value::Boolean(true)
    ));
}

#[test]
fn loose_eq_boolean_null_undefined() {
    // false → 0, null → 0 via object fallback → 0 == 0 → true? No — null
    // falls through object_vs_primitive_eq without matching any pattern → false
    assert!(!loose_eq(&Value::Boolean(false), &Value::Null));
    assert!(!loose_eq(&Value::Boolean(false), &Value::Undefined));
    assert!(!loose_eq(&Value::Boolean(true), &Value::Null));
}

#[test]
fn loose_eq_boolean_object() {
    let mut o = Object::new(ObjectKind::Ordinary);
    o.set("valueOf", native_fn());
    let obj_val = Value::Object(Rc::new(RefCell::new(o)));
    // native_fn returns undefined → false == undefined → 0 == undefined → false
    assert!(!loose_eq(&Value::Boolean(false), &obj_val));
}

// ── loose_eq: object-primitive ───────────────────────────────────────────

#[test]
fn loose_eq_object_number() {
    let o = obj();
    assert!(!loose_eq(&o, &Value::Number(0.0)));
}

#[test]
fn loose_eq_object_string() {
    let o = obj();
    // {} → "[object Object]" → equals the string "[object Object]"
    assert!(loose_eq(&o, &Value::String("[object Object]".into())));
}

#[test]
fn loose_eq_object_object_same() {
    let a = obj();
    let b = obj();
    assert!(!loose_eq(&a, &b), "different objects are not ==");
}

#[test]
fn loose_eq_same_object() {
    let o = obj();
    assert!(loose_eq(&o, &o));
}

// ── loose_eq: bigint ───────────────────────────────────────────────────

#[test]
fn loose_eq_bigint_cross_type() {
    let b = big(42);
    // BigInt has no special loose_eq handling — falls through to false
    assert!(!loose_eq(&b, &Value::String("42".into())));
    assert!(!loose_eq(&Value::String("42".into()), &b));
    assert!(!loose_eq(&b, &Value::Number(42.0)));
}

// ── parse_number_string ─────────────────────────────────────────────────

#[test]
fn parse_number_string_empty() {
    assert_eq!(parse_number_string(""), Some(0.0));
    assert_eq!(parse_number_string("   "), Some(0.0));
}

#[allow(clippy::approx_constant)]
#[test]
fn parse_number_string_decimal() {
    assert_eq!(parse_number_string("42"), Some(42.0));
    assert_eq!(parse_number_string("-17"), Some(-17.0));
    assert_eq!(parse_number_string("3.14"), Some(3.14));
    assert_eq!(parse_number_string("6.022e23"), Some(6.022e23));
}

#[test]
fn parse_number_string_hex() {
    assert_eq!(parse_number_string("0xFF"), Some(255.0));
    assert_eq!(parse_number_string("0Xff"), Some(255.0));
    assert_eq!(parse_number_string("0x0"), Some(0.0));
}

#[test]
fn parse_number_string_binary() {
    assert_eq!(parse_number_string("0b1010"), Some(10.0));
    assert_eq!(parse_number_string("0B0010"), Some(2.0));
}

#[test]
fn parse_number_string_octal() {
    assert_eq!(parse_number_string("0o77"), Some(63.0));
    assert_eq!(parse_number_string("0O77"), Some(63.0));
}

#[test]
fn parse_number_string_invalid() {
    assert_eq!(parse_number_string("abc"), None);
    assert_eq!(parse_number_string(""), Some(0.0));
    assert_eq!(parse_number_string("0x"), None);
    assert_eq!(parse_number_string("0b"), None);
    assert_eq!(parse_number_string("0o"), None);
}

// ── to_primitive_for_compare ────────────────────────────────────────────

#[allow(clippy::approx_constant)]
#[test]
fn to_primitive_for_compare_primitives() {
    assert_eq!(
        to_primitive_for_compare(&Value::Undefined),
        Value::Undefined
    );
    assert_eq!(to_primitive_for_compare(&Value::Null), Value::Null);
    assert_eq!(
        to_primitive_for_compare(&Value::Boolean(true)),
        Value::Boolean(true)
    );
    assert_eq!(
        to_primitive_for_compare(&Value::Number(3.14)),
        Value::Number(3.14)
    );
    assert_eq!(
        to_primitive_for_compare(&Value::String("hi".into())),
        Value::String("hi".into())
    );
    // Symbol: returns unchanged — verify via pointer identity
    let s = sym("x");
    let s2 = sym("x");
    let Value::Symbol(ref s_rc) = s else { panic!() };
    let Value::Symbol(ref s2_rc) = s2 else {
        panic!()
    };
    assert!(!Rc::ptr_eq(s_rc, s2_rc), "sym() creates different Rcs");
    let result = to_primitive_for_compare(&s);
    let Value::Symbol(ref result_rc) = result else {
        panic!("expected Symbol")
    };
    assert!(
        Rc::ptr_eq(result_rc, s_rc),
        "to_primitive_for_compare should return same Symbol"
    );
    // BigInt: ToPrimitive(BigInt) → undefined (no hint, PreferredType is none)
    assert_eq!(to_primitive_for_compare(&big(99)), Value::Undefined);
}

#[test]
fn to_primitive_for_compare_function() {
    let f = func();
    assert_eq!(
        to_primitive_for_compare(&f),
        Value::String("[object Function]".into())
    );
}

#[test]
fn to_primitive_for_compare_native_function() {
    let nf = native_fn();
    assert_eq!(
        to_primitive_for_compare(&nf),
        Value::String("[object Function]".into())
    );
}

#[test]
fn to_primitive_for_compare_native_constructor() {
    let nc = native_ctor();
    assert_eq!(
        to_primitive_for_compare(&nc),
        Value::String("[object Function]".into())
    );
}

#[test]
fn to_primitive_for_compare_object_plain() {
    let o = obj();
    assert_eq!(
        to_primitive_for_compare(&o),
        Value::String("[object Object]".into())
    );
}

#[test]
fn to_primitive_for_compare_object_with_valueof() {
    let mut o = Object::new(ObjectKind::Ordinary);
    o.set("valueOf", native_fn());
    let obj_val = Value::Object(Rc::new(RefCell::new(o)));
    assert_eq!(to_primitive_for_compare(&obj_val), Value::Undefined);
}

#[test]
fn to_primitive_for_compare_object_falls_back_to_object_string() {
    let o = obj();
    assert_eq!(
        to_primitive_for_compare(&o),
        Value::String("[object Object]".into())
    );
}

// ── primitive_for_compare ───────────────────────────────────────────────

#[test]
fn primitive_for_compare_all_primitives() {
    assert_eq!(
        primitive_for_compare(&Value::Undefined),
        Some(Value::Undefined)
    );
    assert_eq!(primitive_for_compare(&Value::Null), Some(Value::Null));
    assert_eq!(
        primitive_for_compare(&Value::Boolean(true)),
        Some(Value::Boolean(true))
    );
    assert_eq!(
        primitive_for_compare(&Value::Number(1.0)),
        Some(Value::Number(1.0))
    );
    assert_eq!(
        primitive_for_compare(&Value::String("x".into())),
        Some(Value::String("x".into()))
    );
    // Symbol: verify pointer identity preserved
    let s = sym("s");
    let result = primitive_for_compare(&s);
    let Some(Value::Symbol(ref result_rc)) = result else {
        panic!("expected Some(Symbol)")
    };
    let Value::Symbol(ref s_rc) = s else {
        panic!("expected Symbol")
    };
    assert!(Rc::ptr_eq(result_rc, s_rc));
    // BigInt: not a primitive type in primitive_for_compare → None
    assert_eq!(primitive_for_compare(&big(5)), None);
}

#[test]
fn primitive_for_compare_object_returns_none() {
    assert_eq!(primitive_for_compare(&obj()), None);
    assert_eq!(primitive_for_compare(&func()), None);
    assert_eq!(primitive_for_compare(&native_fn()), None);
}

// ── to_primitive_for_compare_strict ────────────────────────────────────

#[test]
fn to_primitive_for_compare_strict_ok() {
    assert_eq!(
        to_primitive_for_compare_strict(&Value::Number(42.0)).unwrap(),
        Value::Number(42.0)
    );
    assert_eq!(
        to_primitive_for_compare_strict(&Value::String("hi".into())).unwrap(),
        Value::String("hi".into())
    );
    // For objects that coerce to "[object Object]", strict version also returns Ok
    let o = obj();
    assert_eq!(
        to_primitive_for_compare_strict(&o).unwrap(),
        Value::String("[object Object]".into())
    );
}
