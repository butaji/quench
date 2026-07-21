//! Tests for value/compare.rs

use super::*;
use crate::value::{NativeConstructor, NativeFunction, Object, ObjectKind, Symbol, ValueFunction};
use std::cell::RefCell;
use std::rc::Rc;

fn make_env() -> Rc<RefCell<crate::env::Environment>> {
    Rc::new(RefCell::new(crate::env::Environment::new()))
}

fn make_sym(desc: &str) -> Value {
    Value::Symbol(Rc::new(Symbol {
        desc: Some(desc.into()),
        global: false,
    }))
}

// ─── same_value_numbers ───────────────────────────────────────────────────────

#[test]
fn same_value_numbers_nan_equals_nan() {
    assert!(same_value_numbers(f64::NAN, f64::NAN));
}

#[test]
fn same_value_numbers_positive_zero_not_equals_negative_zero() {
    assert!(!same_value_numbers(0.0, -0.0));
}

#[test]
fn same_value_numbers_negative_zero_not_equals_positive_zero() {
    assert!(!same_value_numbers(-0.0, 0.0));
}

#[test]
fn same_value_numbers_positive_zero_equals_positive_zero() {
    assert!(same_value_numbers(0.0, 0.0));
}

#[test]
fn same_value_numbers_negative_zero_equals_negative_zero() {
    assert!(same_value_numbers(-0.0, -0.0));
}

#[test]
fn same_value_numbers_normal_numbers_equal() {
    assert!(same_value_numbers(1.0, 1.0));
    assert!(same_value_numbers(2.5, 2.5));
    assert!(same_value_numbers(-42.0, -42.0));
}

#[test]
fn same_value_numbers_normal_numbers_not_equal() {
    assert!(!same_value_numbers(1.0, 2.0));
    assert!(!same_value_numbers(-1.0, 1.0));
}

#[test]
fn same_value_numbers_infinity() {
    assert!(same_value_numbers(f64::INFINITY, f64::INFINITY));
    assert!(same_value_numbers(f64::NEG_INFINITY, f64::NEG_INFINITY));
    assert!(!same_value_numbers(f64::INFINITY, f64::NEG_INFINITY));
}

// ─── parse_number_string ──────────────────────────────────────────────────────

#[test]
fn parse_number_string_empty() {
    assert_eq!(parse_number_string(""), Some(0.0));
}

#[test]
fn parse_number_string_whitespace_only() {
    assert_eq!(parse_number_string("   "), Some(0.0));
    assert_eq!(parse_number_string("\t\n"), Some(0.0));
}

#[test]
fn parse_number_string_normal_decimal() {
    assert_eq!(parse_number_string("42"), Some(42.0));
    assert_eq!(parse_number_string("-7.5"), Some(-7.5));
    assert_eq!(parse_number_string("0"), Some(0.0));
    assert_eq!(parse_number_string("123.456"), Some(123.456));
}

#[test]
fn parse_number_string_hex() {
    assert_eq!(parse_number_string("0xFF"), Some(255.0));
    assert_eq!(parse_number_string("0X1F"), Some(31.0));
    assert_eq!(parse_number_string("0x0"), Some(0.0));
    assert_eq!(parse_number_string("0xABC"), Some(2748.0));
}

#[test]
fn parse_number_string_binary() {
    assert_eq!(parse_number_string("0b101"), Some(5.0));
    assert_eq!(parse_number_string("0B1"), Some(1.0));
    assert_eq!(parse_number_string("0b0"), Some(0.0));
    assert_eq!(parse_number_string("0b1111"), Some(15.0));
}

#[test]
fn parse_number_string_octal() {
    assert_eq!(parse_number_string("0o77"), Some(63.0));
    assert_eq!(parse_number_string("0O10"), Some(8.0));
    assert_eq!(parse_number_string("0o0"), Some(0.0));
    assert_eq!(parse_number_string("0o17"), Some(15.0));
}

#[test]
fn parse_number_string_whitespace_trimmed() {
    assert_eq!(parse_number_string("  42  "), Some(42.0));
    assert_eq!(parse_number_string("\t123\n"), Some(123.0));
}

#[test]
fn parse_number_string_invalid() {
    assert_eq!(parse_number_string("hello"), None);
    assert_eq!(parse_number_string("abc123"), None);
}

#[test]
fn parse_number_string_edge_missing_digits() {
    assert_eq!(parse_number_string("0x"), None);
    assert_eq!(parse_number_string("0b"), None);
    assert_eq!(parse_number_string("0o"), None);
}

#[test]
fn parse_number_string_scientific() {
    assert_eq!(parse_number_string("1e3"), Some(1000.0));
    assert_eq!(parse_number_string("2.5e2"), Some(250.0));
    assert_eq!(parse_number_string("1E-1"), Some(0.1));
}

// ─── strict_eq ────────────────────────────────────────────────────────────────

#[test]
fn strict_eq_undefined() {
    assert!(strict_eq(&Value::Undefined, &Value::Undefined));
    assert!(!strict_eq(&Value::Undefined, &Value::Null));
}

#[test]
fn strict_eq_null() {
    assert!(strict_eq(&Value::Null, &Value::Null));
    assert!(!strict_eq(&Value::Null, &Value::Undefined));
}

#[test]
fn strict_eq_boolean() {
    assert!(strict_eq(&Value::Boolean(true), &Value::Boolean(true)));
    assert!(strict_eq(&Value::Boolean(false), &Value::Boolean(false)));
    assert!(!strict_eq(&Value::Boolean(true), &Value::Boolean(false)));
}

#[test]
fn strict_eq_number() {
    assert!(strict_eq(&Value::Number(42.0), &Value::Number(42.0)));
    assert!(!strict_eq(&Value::Number(1.0), &Value::Number(2.0)));
    assert!(strict_eq(&Value::Number(0.0), &Value::Number(-0.0)));
}

#[test]
fn strict_eq_string() {
    assert!(strict_eq(
        &Value::String("hello".into()),
        &Value::String("hello".into())
    ));
    assert!(!strict_eq(
        &Value::String("hello".into()),
        &Value::String("world".into())
    ));
}

#[test]
fn strict_eq_symbol_same_rc() {
    let sym = make_sym("test");
    let a = sym.clone();
    let b = sym.clone();
    assert!(strict_eq(&a, &b));
}

#[test]
fn strict_eq_symbol_different_rc() {
    let a = make_sym("test");
    let b = make_sym("test");
    assert!(!strict_eq(&a, &b));
}

#[test]
fn strict_eq_object_same_rc() {
    let obj = Rc::new(RefCell::new(Object::new(ObjectKind::Ordinary)));
    let a = Value::Object(Rc::clone(&obj));
    let b = Value::Object(Rc::clone(&obj));
    assert!(strict_eq(&a, &b));
}

#[test]
fn strict_eq_object_different_rc() {
    let a = Value::Object(Rc::new(RefCell::new(Object::new(ObjectKind::Ordinary))));
    let b = Value::Object(Rc::new(RefCell::new(Object::new(ObjectKind::Ordinary))));
    assert!(!strict_eq(&a, &b));
}

#[test]
fn strict_eq_bigint() {
    use num_bigint::BigInt;
    let a = Value::BigInt(Rc::new(BigInt::from(123)));
    let b = Value::BigInt(Rc::new(BigInt::from(123)));
    let c = Value::BigInt(Rc::new(BigInt::from(456)));
    assert!(strict_eq(&a, &b));
    assert!(!strict_eq(&a, &c));
}

#[test]
fn strict_eq_function_same_identity() {
    let env = make_env();
    let f1 = ValueFunction::new(Some("f".into()), vec![], vec![], env, false, false);
    let f2 = f1.clone();
    let a = Value::Function(f1);
    let b = Value::Function(f2);
    assert!(strict_eq(&a, &b));
}

#[test]
fn strict_eq_function_different_identity() {
    let env = make_env();
    let f1 = ValueFunction::new(Some("f1".into()), vec![], vec![], env.clone(), false, false);
    let f2 = ValueFunction::new(Some("f2".into()), vec![], vec![], env, false, false);
    let a = Value::Function(f1);
    let b = Value::Function(f2);
    assert!(!strict_eq(&a, &b));
}

#[test]
fn strict_eq_native_function_same_reference() {
    let nf1 = Value::NativeFunction(Rc::new(NativeFunction::new(|_| Ok(Value::Undefined))));
    let nf2 = Value::NativeFunction(Rc::new(NativeFunction::new(|_| Ok(Value::Undefined))));
    let nf1_clone = nf1.clone();
    assert!(strict_eq(&nf1, &nf1_clone));
    assert!(!strict_eq(&nf1, &nf2));
}

#[test]
fn strict_eq_native_constructor_same_reference() {
    let proto = Rc::new(RefCell::new(Object::new(ObjectKind::Ordinary)));
    let nc1 = Value::NativeConstructor(Rc::new(NativeConstructor::new(
        |_| Ok(Value::Undefined),
        Rc::clone(&proto),
    )));
    let nc1_clone = nc1.clone();
    assert!(strict_eq(&nc1, &nc1_clone));
}

#[test]
fn strict_eq_class_different_identity() {
    let a = Value::Class(Box::new(crate::value::ClassValue::from_ast(
        &crate::ast::Class {
            name: Some("C1".into()),
            super_class: None,
            body: vec![],
        },
    )));
    let b = Value::Class(Box::new(crate::value::ClassValue::from_ast(
        &crate::ast::Class {
            name: Some("C2".into()),
            super_class: None,
            body: vec![],
        },
    )));
    assert!(!strict_eq(&a, &b));
}

#[test]
fn strict_eq_different_types_null_undefined() {
    assert!(!strict_eq(&Value::Null, &Value::Undefined));
    assert!(strict_eq(&Value::Null, &Value::Null));
    assert!(strict_eq(&Value::Undefined, &Value::Undefined));
}

#[test]
fn strict_eq_cross_type_all_false_except_null_undefined() {
    assert!(!strict_eq(&Value::Null, &Value::Boolean(false)));
    assert!(!strict_eq(&Value::Undefined, &Value::Number(0.0)));
    assert!(!strict_eq(&Value::Boolean(true), &Value::Number(1.0)));
    assert!(!strict_eq(&Value::Number(0.0), &Value::String("0".into())));
}

// ─── same_value ────────────────────────────────────────────────────────────────

#[test]
fn same_value_nan_equals_nan() {
    let a = Value::Number(f64::NAN);
    let b = Value::Number(f64::NAN);
    assert!(same_value(&a, &b));
}

#[test]
fn same_value_positive_zero_not_equals_negative_zero() {
    let a = Value::Number(0.0);
    let b = Value::Number(-0.0);
    assert!(!same_value(&a, &b));
}

#[test]
fn same_value_negative_zero_not_equals_positive_zero() {
    let a = Value::Number(-0.0);
    let b = Value::Number(0.0);
    assert!(!same_value(&a, &b));
}

#[test]
fn same_value_normal_numbers() {
    assert!(same_value(&Value::Number(1.0), &Value::Number(1.0)));
    assert!(!same_value(&Value::Number(1.0), &Value::Number(2.0)));
}

#[test]
fn same_value_different_type_returns_false() {
    assert!(!same_value(&Value::Number(1.0), &Value::String("1".into())));
    assert!(!same_value(&Value::Boolean(true), &Value::Number(1.0)));
}

#[test]
fn same_value_function_same_identity() {
    let env = make_env();
    let f1 = ValueFunction::new(Some("f".into()), vec![], vec![], env, false, false);
    let f2 = f1.clone();
    assert!(same_value(&Value::Function(f1), &Value::Function(f2)));
}

// ─── loose_eq ────────────────────────────────────────────────────────────────

#[test]
fn loose_eq_null_undefined() {
    assert!(loose_eq(&Value::Null, &Value::Undefined));
    assert!(loose_eq(&Value::Undefined, &Value::Null));
}

#[test]
fn loose_eq_number_string() {
    assert!(loose_eq(&Value::String("42".into()), &Value::Number(42.0)));
    assert!(loose_eq(&Value::Number(42.0), &Value::String("42".into())));
}

#[test]
fn loose_eq_boolean_coercion() {
    assert!(loose_eq(&Value::Boolean(true), &Value::String("1".into())));
    assert!(loose_eq(&Value::Boolean(false), &Value::String("0".into())));
}

#[test]
fn loose_eq_undefined_not_equal_to_zero() {
    assert!(!loose_eq(&Value::Undefined, &Value::Number(0.0)));
    assert!(!loose_eq(&Value::Null, &Value::Number(0.0)));
}

#[test]
fn loose_eq_same_type_numbers() {
    assert!(loose_eq(&Value::Number(1.0), &Value::Number(1.0)));
    assert!(!loose_eq(&Value::Number(1.0), &Value::Number(2.0)));
}

// ─── primitive_for_compare ────────────────────────────────────────────────────

#[test]
fn primitive_for_compare_undefined() {
    assert_eq!(
        primitive_for_compare(&Value::Undefined),
        Some(Value::Undefined)
    );
}

#[test]
fn primitive_for_compare_null() {
    assert_eq!(primitive_for_compare(&Value::Null), Some(Value::Null));
}

#[test]
fn primitive_for_compare_boolean() {
    assert_eq!(
        primitive_for_compare(&Value::Boolean(true)),
        Some(Value::Boolean(true))
    );
    assert_eq!(
        primitive_for_compare(&Value::Boolean(false)),
        Some(Value::Boolean(false))
    );
}

#[test]
fn primitive_for_compare_number() {
    assert_eq!(
        primitive_for_compare(&Value::Number(42.0)),
        Some(Value::Number(42.0))
    );
}

#[test]
fn primitive_for_compare_string() {
    assert_eq!(
        primitive_for_compare(&Value::String("hello".into())),
        Some(Value::String("hello".into()))
    );
}

#[test]
fn primitive_for_compare_symbol() {
    let val = make_sym("test");
    let result = primitive_for_compare(&val);
    assert!(matches!(result, Some(Value::Symbol(_))));
}

#[test]
fn primitive_for_compare_object() {
    let obj = Value::Object(Rc::new(RefCell::new(Object::new(ObjectKind::Ordinary))));
    assert_eq!(primitive_for_compare(&obj), None);
}

#[test]
fn primitive_for_compare_function() {
    let env = make_env();
    let f = ValueFunction::new(Some("f".into()), vec![], vec![], env, false, false);
    assert_eq!(primitive_for_compare(&Value::Function(f)), None);
}

#[test]
fn primitive_for_compare_native_function() {
    let nf = Value::NativeFunction(Rc::new(NativeFunction::new(|_| Ok(Value::Undefined))));
    assert_eq!(primitive_for_compare(&nf), None);
}

#[test]
fn primitive_for_compare_native_constructor() {
    let proto = Rc::new(RefCell::new(Object::new(ObjectKind::Ordinary)));
    let nc = Value::NativeConstructor(Rc::new(NativeConstructor::new(
        |_| Ok(Value::Undefined),
        proto,
    )));
    assert_eq!(primitive_for_compare(&nc), None);
}

#[test]
fn primitive_for_compare_bigint() {
    use num_bigint::BigInt;
    let bi = Value::BigInt(Rc::new(BigInt::from(123)));
    assert_eq!(primitive_for_compare(&bi), None);
}

// ─── to_primitive_for_compare_strict ────────────────────────────────────────

#[test]
fn to_primitive_for_compare_strict_primitives() {
    assert_eq!(
        to_primitive_for_compare_strict(&Value::Number(42.0)).unwrap(),
        Value::Number(42.0)
    );
    assert_eq!(
        to_primitive_for_compare_strict(&Value::String("hello".into())).unwrap(),
        Value::String("hello".into())
    );
}

// ─── strict_eq cross-type comparisons ────────────────────────────────────────

#[test]
fn strict_eq_undefined_vs_null() {
    assert!(!strict_eq(&Value::Undefined, &Value::Null));
    assert!(!strict_eq(&Value::Null, &Value::Undefined));
}

#[test]
fn strict_eq_number_vs_string() {
    assert!(!strict_eq(
        &Value::Number(42.0),
        &Value::String("42".to_string())
    ));
    assert!(!strict_eq(
        &Value::String("42".to_string()),
        &Value::Number(42.0)
    ));
}

#[test]
fn strict_eq_boolean_vs_number() {
    assert!(!strict_eq(&Value::Boolean(true), &Value::Number(1.0)));
    assert!(!strict_eq(&Value::Boolean(false), &Value::Number(0.0)));
}

#[test]
fn strict_eq_nan() {
    // NaN !== NaN (per spec)
    let nan = Value::Number(f64::NAN);
    assert!(!strict_eq(&nan, &nan));
}

#[test]
fn strict_eq_positive_zero_negative_zero() {
    // +0 === -0 per strict equality
    assert!(strict_eq(&Value::Number(0.0), &Value::Number(-0.0)));
}
