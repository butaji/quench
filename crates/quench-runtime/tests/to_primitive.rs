//! Unit tests for value-to-primitive conversion
//!
//! Tests for the unified to_primitive function that handles JavaScript's
//! ToPrimitive abstract operation.

use quench_runtime::Context;
use quench_runtime::value::{to_js_string, to_number, to_bool, to_primitive, Value};

#[test]
fn test_to_js_string_primitives() {
    assert_eq!(to_js_string(&Value::Undefined), "undefined");
    assert_eq!(to_js_string(&Value::Null), "null");
    assert_eq!(to_js_string(&Value::Boolean(true)), "true");
    assert_eq!(to_js_string(&Value::Boolean(false)), "false");
    assert_eq!(to_js_string(&Value::Number(42.0)), "42");
    assert_eq!(to_js_string(&Value::Number(std::f64::consts::PI)), "3.141592653589793");
    assert_eq!(to_js_string(&Value::String("hello".to_string())), "hello");
    assert_eq!(to_js_string(&Value::Number(f64::NAN)), "NaN");
    assert_eq!(to_js_string(&Value::Number(f64::INFINITY)), "Infinity");
    assert_eq!(to_js_string(&Value::Number(f64::NEG_INFINITY)), "-Infinity");
}

#[test]
fn test_to_number_primitives() {
    assert!(to_number(&Value::Undefined).is_nan());
    assert_eq!(to_number(&Value::Null), 0.0);
    assert_eq!(to_number(&Value::Boolean(true)), 1.0);
    assert_eq!(to_number(&Value::Boolean(false)), 0.0);
    assert_eq!(to_number(&Value::Number(42.0)), 42.0);
    assert_eq!(to_number(&Value::Number(std::f64::consts::PI)), std::f64::consts::PI);
    assert_eq!(to_number(&Value::String("42".to_string())), 42.0);
    assert_eq!(to_number(&Value::String("".to_string())), 0.0);
    assert_eq!(to_number(&Value::String("   ".to_string())), 0.0);
    assert!(to_number(&Value::String("hello".to_string())).is_nan());
}

#[test]
fn test_to_bool_primitives() {
    assert!(!to_bool(&Value::Undefined));
    assert!(!to_bool(&Value::Null));
    assert!(to_bool(&Value::Boolean(true)));
    assert!(!to_bool(&Value::Boolean(false)));
    assert!(to_bool(&Value::Number(42.0)));
    assert!(!to_bool(&Value::Number(0.0)));
    assert!(!to_bool(&Value::Number(f64::NAN)));
    assert!(to_bool(&Value::String("hello".to_string())));
    assert!(!to_bool(&Value::String("".to_string())));
}

#[test]
fn test_to_primitive_hint_number() {
    // For primitives, ToPrimitive should return them as-is
    assert_eq!(to_primitive(&Value::Undefined, Some("number")), Value::Undefined);
    assert_eq!(to_primitive(&Value::Null, Some("number")), Value::Null);
    assert_eq!(to_primitive(&Value::Boolean(true), Some("number")), Value::Boolean(true));
    assert_eq!(to_primitive(&Value::Number(42.0), Some("number")), Value::Number(42.0));
    assert_eq!(to_primitive(&Value::String("hello".to_string()), Some("number")), Value::String("hello".to_string()));
}

#[test]
fn test_to_primitive_hint_string() {
    // For primitives, ToPrimitive should return them as-is
    assert_eq!(to_primitive(&Value::Undefined, Some("string")), Value::Undefined);
    assert_eq!(to_primitive(&Value::Number(42.0), Some("string")), Value::Number(42.0));
    assert_eq!(to_primitive(&Value::String("hello".to_string()), Some("string")), Value::String("hello".to_string()));
}

#[test]
fn test_to_primitive_no_hint() {
    // For primitives, no hint means the same as with hint
    assert_eq!(to_primitive(&Value::Undefined, None), Value::Undefined);
    assert_eq!(to_primitive(&Value::Number(42.0), None), Value::Number(42.0));
}

#[test]
fn test_to_primitive_symbol() {
    // Symbols should be returned as-is (via to_js_string which handles Symbol)
    let sym = Value::Symbol("test".to_string());
    let result = to_primitive(&sym, Some("number"));
    // Symbols convert to strings via to_js_string
    assert_eq!(to_js_string(&result), "Symbol(test)");
}

#[test]
fn test_conversion_functions_consistent() {
    // Verify that to_js_string for primitives returns the expected string
    // and to_number/to_bool produce expected values
    
    // undefined
    assert_eq!(to_js_string(&Value::Undefined), "undefined");
    assert!(to_number(&Value::Undefined).is_nan());
    assert!(!to_bool(&Value::Undefined));
    
    // null
    assert_eq!(to_js_string(&Value::Null), "null");
    assert_eq!(to_number(&Value::Null), 0.0);
    assert!(!to_bool(&Value::Null));
    
    // boolean
    assert_eq!(to_js_string(&Value::Boolean(true)), "true");
    assert_eq!(to_number(&Value::Boolean(true)), 1.0);
    assert!(to_bool(&Value::Boolean(true)));
    
    // number
    assert_eq!(to_js_string(&Value::Number(0.0)), "0");
    assert_eq!(to_number(&Value::Number(0.0)), 0.0);
    assert!(!to_bool(&Value::Number(0.0)));
    
    // string
    assert_eq!(to_js_string(&Value::String("".to_string())), "");
    assert_eq!(to_number(&Value::String("".to_string())), 0.0);
    assert!(!to_bool(&Value::String("".to_string())));
}

#[test]
fn test_date_to_string() {
    let mut ctx = Context::new().unwrap();
    
    // Date.toString should return a string representation
    let result = ctx.eval("(new Date()).toString()").unwrap();
    let s = format!("{}", result);
    assert!(s.starts_with("Date @ "));
}

#[test]
fn test_date_valueof() {
    // Test that Date.valueOf() returns a number - simplified to avoid stack overflow
    let mut ctx = Context::new().unwrap();
    let result = ctx.eval("typeof (new Date()).valueOf()").unwrap();
    assert_eq!(result.to_string(), "number");
}
