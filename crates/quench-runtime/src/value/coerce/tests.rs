//! Unit tests for coerce.rs — number_to_string, string_to_number, to_js_string,
//! to_number, to_uint32 via the public API.

use std::cell::RefCell;
use std::rc::Rc;

use crate::value::coerce::simple_string_value;
use crate::value::convert::{to_js_string, to_number, to_uint32};
use crate::value::kind::ObjectKind;
use crate::value::object::Object;
use crate::value::{NativeConstructor, NativeFunction, Symbol, Value};

// ── number_to_string via to_js_string ─────────────────────────────────────────

#[test]
fn test_number_to_string_nan() {
    assert_eq!(to_js_string(&Value::Number(f64::NAN)), "NaN");
}

#[test]
fn test_number_to_string_infinity() {
    assert_eq!(to_js_string(&Value::Number(f64::INFINITY)), "Infinity");
}

#[test]
fn test_number_to_string_neg_infinity() {
    assert_eq!(to_js_string(&Value::Number(f64::NEG_INFINITY)), "-Infinity");
}

#[test]
fn test_number_to_string_positive_zero() {
    assert_eq!(to_js_string(&Value::Number(0.0)), "0");
}

#[test]
fn test_number_to_string_negative_zero() {
    assert_eq!(to_js_string(&Value::Number(-0.0)), "0");
}

#[test]
fn test_number_to_string_integer_small() {
    assert_eq!(to_js_string(&Value::Number(42.0)), "42");
    assert_eq!(to_js_string(&Value::Number(1_000_000.0)), "1000000");
    assert_eq!(
        to_js_string(&Value::Number(999_999_999_999_999.0)),
        "999999999999999"
    );
}

#[test]
fn test_number_to_string_float() {
    assert_eq!(to_js_string(&Value::Number(1.5)), "1.5");
    assert_eq!(to_js_string(&Value::Number(-(157.0_f64 / 50.0))), "-3.14");
}

#[test]
fn test_number_to_string_scientific() {
    assert_eq!(to_js_string(&Value::Number(1e20)), "100000000000000000000");
}

// ── string_to_number via to_number ────────────────────────────────────────────

#[test]
fn test_string_to_number_hex_lowercase() {
    assert_eq!(to_number(&Value::String("0xff".to_string())), 255.0);
}

#[test]
fn test_string_to_number_hex_uppercase() {
    assert_eq!(to_number(&Value::String("0XFF".to_string())), 255.0);
}

#[test]
fn test_string_to_number_binary() {
    assert_eq!(to_number(&Value::String("0b10".to_string())), 2.0);
    assert_eq!(to_number(&Value::String("0B1010".to_string())), 10.0);
}

#[test]
fn test_string_to_number_octal() {
    assert_eq!(to_number(&Value::String("0o17".to_string())), 15.0);
    assert_eq!(to_number(&Value::String("0O777".to_string())), 511.0);
}

#[test]
fn test_string_to_number_invalid_hex_returns_nan() {
    assert!(to_number(&Value::String("0xGG".to_string())).is_nan());
}

#[test]
fn test_string_to_number_invalid_binary_returns_nan() {
    assert!(to_number(&Value::String("0b123".to_string())).is_nan());
}

#[test]
fn test_string_to_number_invalid_octal_returns_nan() {
    assert!(to_number(&Value::String("0o89".to_string())).is_nan());
}

#[test]
fn test_string_to_number_empty_is_zero() {
    assert_eq!(to_number(&Value::String("".to_string())), 0.0);
}

#[test]
fn test_string_to_number_whitespace_only_is_zero() {
    assert_eq!(to_number(&Value::String("   ".to_string())), 0.0);
}

#[test]
fn test_string_to_number_leading_trailing_whitespace() {
    assert_eq!(to_number(&Value::String("  42  ".to_string())), 42.0);
}

#[test]
fn test_string_to_number_explicit_infinity() {
    assert_eq!(
        to_number(&Value::String("Infinity".to_string())),
        f64::INFINITY
    );
    assert_eq!(
        to_number(&Value::String("-Infinity".to_string())),
        f64::NEG_INFINITY
    );
}

#[test]
fn test_string_to_number_explicit_nan() {
    assert!(to_number(&Value::String("NaN".to_string())).is_nan());
}

#[test]
fn test_string_to_number_float_literal() {
    assert_eq!(
        to_number(&Value::String("3.14".to_string())),
        (157.0_f64 / 50.0)
    );
    assert_eq!(
        to_number(&Value::String("-0.5".to_string())),
        -(1.0_f64 / 2.0)
    );
}

#[test]
fn test_string_to_number_scientific_literal() {
    assert_eq!(to_number(&Value::String("1e3".to_string())), 1000.0);
    assert_eq!(to_number(&Value::String("2.5e-2".to_string())), 0.025);
}

#[test]
fn test_string_to_number_invalid_returns_nan() {
    assert!(to_number(&Value::String("hello".to_string())).is_nan());
    assert!(to_number(&Value::String("true".to_string())).is_nan());
}

// ── simple_string_value ───────────────────────────────────────────────────────

#[test]
fn test_simple_string_value_undefined() {
    assert_eq!(
        simple_string_value(&Value::Undefined),
        Some("undefined".to_string())
    );
}

#[test]
fn test_simple_string_value_null() {
    assert_eq!(simple_string_value(&Value::Null), Some("null".to_string()));
}

#[test]
fn test_simple_string_value_boolean() {
    assert_eq!(
        simple_string_value(&Value::Boolean(true)),
        Some("true".to_string())
    );
    assert_eq!(
        simple_string_value(&Value::Boolean(false)),
        Some("false".to_string())
    );
}

#[test]
fn test_simple_string_value_number() {
    assert_eq!(
        simple_string_value(&Value::Number(42.0)),
        Some("42".to_string())
    );
    assert!(simple_string_value(&Value::Number(f64::NAN)).is_some());
}

#[test]
fn test_simple_string_value_string() {
    assert_eq!(
        simple_string_value(&Value::String("hello".to_string())),
        Some("hello".to_string())
    );
}

#[test]
fn test_simple_string_value_bigint() {
    let bi = num_bigint::BigInt::from(123);
    assert_eq!(
        simple_string_value(&Value::BigInt(Rc::new(bi))),
        Some("123n".to_string())
    );
}

#[test]
fn test_simple_string_value_object_returns_none() {
    let obj = Value::Object(Rc::new(RefCell::new(Object::new(ObjectKind::Ordinary))));
    assert_eq!(simple_string_value(&obj), None);
}

// ── to_js_string ───────────────────────────────────────────────────────────────

#[test]
fn test_to_js_string_function() {
    let nf = Value::NativeFunction(Rc::new(NativeFunction::new(|_| Ok(Value::Undefined))));
    assert_eq!(to_js_string(&nf), "[Function]");
}

#[test]
fn test_to_js_string_class() {
    let proto = Rc::new(RefCell::new(Object::new(ObjectKind::Ordinary)));
    let nc = Value::NativeConstructor(Rc::new(NativeConstructor::new(
        |_| Ok(Value::Undefined),
        Rc::clone(&proto),
    )));
    assert_eq!(to_js_string(&nc), "[Function]");
}

#[test]
fn test_to_js_string_value_function() {
    // AST-based Value::Function should use source_text, not fall through to "undefined"
    let func = Value::Function(crate::value::ValueFunction::new(
        Some("foo".to_string()),
        vec![],
        Vec::new(),
        std::rc::Rc::new(std::cell::RefCell::new(crate::env::Environment::new())),
        false,
        false,
    ));
    let result = to_js_string(&func);
    assert!(
        result.contains("foo"),
        "to_js_string for Value::Function should contain 'foo', got: {}",
        result
    );
    assert!(
        !result.contains("undefined"),
        "to_js_string for Value::Function should NOT contain 'undefined', got: {}",
        result
    );
}

#[test]
fn test_to_js_string_value_function_with_body() {
    // AST-based Value::Function with body should show the body in source_text
    use crate::ast::{Expression, Statement};
    let body = vec![Statement::Return(Some(Box::new(Expression::Number(42.0))))];
    let func = Value::Function(crate::value::ValueFunction::new(
        Some("bar".to_string()),
        vec![],
        body,
        std::rc::Rc::new(std::cell::RefCell::new(crate::env::Environment::new())),
        false,
        false,
    ));
    let result = to_js_string(&func);
    assert!(
        result.contains("bar"),
        "to_js_string for Value::Function with body should contain 'bar', got: {}",
        result
    );
}

#[test]
fn test_to_js_string_symbol_with_desc() {
    let sym = Value::Symbol(Rc::new(Symbol {
        desc: Some("myDesc".into()),
        global: false,
    }));
    assert_eq!(to_js_string(&sym), "Symbol(myDesc)");
}

#[test]
fn test_to_js_string_symbol_no_desc() {
    let sym = Value::Symbol(Rc::new(Symbol {
        desc: None,
        global: false,
    }));
    assert_eq!(to_js_string(&sym), "Symbol()");
}

#[test]
fn test_to_js_string_undefined() {
    assert_eq!(to_js_string(&Value::Undefined), "undefined");
}

#[test]
fn test_to_js_string_null() {
    assert_eq!(to_js_string(&Value::Null), "null");
}

// ── to_number ─────────────────────────────────────────────────────────────────

#[test]
fn test_to_number_bigint_returns_nan() {
    // Per ES spec, ToNumber(BigInt) is not a simple coercion — BigInt-to-Number
    // throws via BigInt::toNumber or Number() with BigInt arg. Our runtime handles
    // this via BigInt::to_number JS builtin; raw to_number returns NaN.
    let bi = num_bigint::BigInt::from(42);
    assert!(to_number(&Value::BigInt(Rc::new(bi))).is_nan());
}

#[test]
fn test_to_number_function_is_nan() {
    let nf = Value::NativeFunction(Rc::new(NativeFunction::new(|_| Ok(Value::Undefined))));
    assert!(to_number(&nf).is_nan());
}

#[test]
fn test_to_number_symbol_is_nan() {
    let sym = Value::Symbol(Rc::new(Symbol {
        desc: Some("test".into()),
        global: false,
    }));
    // to_number on Symbol returns NaN (per ES spec, ToNumber throws)
    assert!(to_number(&sym).is_nan());
}

// ── to_uint32 ─────────────────────────────────────────────────────────────────

#[test]
fn test_to_uint32_fractional_truncates() {
    assert_eq!(to_uint32(1.9), 1);
}

#[test]
fn test_to_uint32_large_positive_wraps() {
    // 2^33 wraps to 0
    assert_eq!(to_uint32(8589934592.0), 0);
}

#[test]
fn test_to_uint32_negative_wraps() {
    assert_eq!(to_uint32(-2.0), 0xFFFFFFFE);
}

#[test]
fn test_to_uint32_very_negative_wraps() {
    // -1.1 truncates to -1, wraps to 2^32-1
    assert_eq!(to_uint32(-1.1), 0xFFFFFFFF);
}

// ── to_bool ───────────────────────────────────────────────────────────────

#[test]
fn test_to_bool_undefined_null() {
    assert!(!crate::value::to_bool(&Value::Undefined));
    assert!(!crate::value::to_bool(&Value::Null));
}

#[test]
fn test_to_bool_boolean() {
    assert!(!crate::value::to_bool(&Value::Boolean(false)));
    assert!(crate::value::to_bool(&Value::Boolean(true)));
}

#[test]
fn test_to_bool_number() {
    assert!(!crate::value::to_bool(&Value::Number(0.0)));
    assert!(!crate::value::to_bool(&Value::Number(-0.0)));
    assert!(!crate::value::to_bool(&Value::Number(f64::NAN)));
    assert!(crate::value::to_bool(&Value::Number(1.0)));
    assert!(crate::value::to_bool(&Value::Number(-1.0)));
    assert!(crate::value::to_bool(&Value::Number(f64::INFINITY)));
    assert!(crate::value::to_bool(&Value::Number(f64::NEG_INFINITY)));
}

#[test]
fn test_to_bool_string() {
    assert!(!crate::value::to_bool(&Value::String(String::new())));
    assert!(crate::value::to_bool(&Value::String("hello".to_string())));
    assert!(crate::value::to_bool(&Value::String("false".to_string())));
    assert!(crate::value::to_bool(&Value::String("0".to_string())));
}

#[test]
fn test_to_bool_object_function_symbol_bigint() {
    let obj = Value::Object(Rc::new(RefCell::new(Object::new(ObjectKind::Ordinary))));
    assert!(crate::value::to_bool(&obj));
    let func = Value::Function(crate::value::ValueFunction::new(
        None,
        vec![],
        Vec::new(),
        std::rc::Rc::new(std::cell::RefCell::new(crate::env::Environment::new())),
        false,
        false,
    ));
    assert!(crate::value::to_bool(&func));
    assert!(crate::value::to_bool(&Value::Symbol(std::rc::Rc::new(
        crate::value::Symbol {
            desc: None,
            global: false
        }
    ))));
    assert!(!crate::value::to_bool(&Value::BigInt(std::rc::Rc::new(
        num_bigint::BigInt::from(0i64)
    ))));
    assert!(crate::value::to_bool(&Value::BigInt(std::rc::Rc::new(
        num_bigint::BigInt::from(1i64)
    ))));
}

// ── try_to_number ─────────────────────────────────────────────────────────

#[test]
fn test_try_to_number_undefined() {
    // to_number_complex returns Ok(NaN) for Undefined (no error path)
    let result = crate::value::convert::try_to_number(&Value::Undefined).unwrap();
    assert!(result.is_nan());
}

#[test]
fn test_try_to_number_null() {
    assert_eq!(
        crate::value::convert::try_to_number(&Value::Null).unwrap(),
        0.0
    );
}

#[test]
fn test_try_to_number_boolean() {
    assert_eq!(
        crate::value::convert::try_to_number(&Value::Boolean(true)).unwrap(),
        1.0
    );
    assert_eq!(
        crate::value::convert::try_to_number(&Value::Boolean(false)).unwrap(),
        0.0
    );
}

#[test]
fn test_try_to_number_numeric_string() {
    assert_eq!(
        crate::value::convert::try_to_number(&Value::String("42".to_string())).unwrap(),
        42.0
    );
}

#[test]
fn test_try_to_number_invalid_string() {
    // to_number_complex returns Ok(NaN) for non-numeric strings
    let result =
        crate::value::convert::try_to_number(&Value::String("not a number".to_string())).unwrap();
    assert!(result.is_nan());
}

// ── to_number_unchecked ───────────────────────────────────────────────────

#[test]
fn test_to_number_unchecked_primitive() {
    assert!(crate::value::convert::to_number_unchecked(&Value::Undefined).is_nan());
    assert_eq!(
        crate::value::convert::to_number_unchecked(&Value::Null),
        0.0
    );
    assert_eq!(
        crate::value::convert::to_number_unchecked(&Value::Boolean(true)),
        1.0
    );
    assert_eq!(
        crate::value::convert::to_number_unchecked(&Value::Number(42.0)),
        42.0
    );
}
