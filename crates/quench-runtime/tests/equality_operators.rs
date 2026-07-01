//! Unit tests for equality operators (==, ===, instanceof, in)
//!
//! Tests for the interpreter's implementation of JavaScript equality operators.

#![allow(unknown_lints)]

use quench_runtime::Context;
use quench_runtime::Value;

// ============================================================================
// Loose Equality (==) Tests
// ============================================================================

#[test]
fn test_loose_eq_null_undefined() {
    let mut ctx = Context::new().unwrap();
    
    // null == undefined should be true
    let result = ctx.eval("null == undefined").unwrap();
    assert_eq!(result, Value::Boolean(true));
    
    // undefined == null should be true
    let result = ctx.eval("undefined == null").unwrap();
    assert_eq!(result, Value::Boolean(true));
}

#[test]
fn test_loose_eq_primitives() {
    let mut ctx = Context::new().unwrap();
    
    // Same primitives
    let result = ctx.eval("5 == 5").unwrap();
    assert_eq!(result, Value::Boolean(true));
    
    let result = ctx.eval("5 == 6").unwrap();
    assert_eq!(result, Value::Boolean(false));
    
    let result = ctx.eval("\"hello\" == \"hello\"").unwrap();
    assert_eq!(result, Value::Boolean(true));
    
    let result = ctx.eval("true == true").unwrap();
    assert_eq!(result, Value::Boolean(true));
}

#[test]
fn test_loose_eq_number_string() {
    let mut ctx = Context::new().unwrap();
    
    // Number == String: coerces string to number
    let result = ctx.eval("5 == \"5\"").unwrap();
    assert_eq!(result, Value::Boolean(true));
    
    let result = ctx.eval("\"5\" == 5").unwrap();
    assert_eq!(result, Value::Boolean(true));
    
    let result = ctx.eval("123 == \"123\"").unwrap();
    assert_eq!(result, Value::Boolean(true));
    
    let result = ctx.eval("1 == \"1.0\"").unwrap();
    assert_eq!(result, Value::Boolean(true));
}

#[test]
fn test_loose_eq_boolean_coercion() {
    let mut ctx = Context::new().unwrap();
    
    // true == 1 (both coerced to 1)
    let result = ctx.eval("true == 1").unwrap();
    assert_eq!(result, Value::Boolean(true));
    
    // false == 0 (both coerced to 0)
    let result = ctx.eval("false == 0").unwrap();
    assert_eq!(result, Value::Boolean(true));
    
    // true == \"1\" (true -> 1, \"1\" -> 1)
    let result = ctx.eval("true == \"1\"").unwrap();
    assert_eq!(result, Value::Boolean(true));
}

#[test]
fn test_loose_eq_inequality() {
    let mut ctx = Context::new().unwrap();
    
    // Different types should not be loosely equal (unless special cases)
    let result = ctx.eval("5 != \"5\"").unwrap();
    assert_eq!(result, Value::Boolean(false)); // They're equal, so != is false
    
    let result = ctx.eval("null != undefined").unwrap();
    assert_eq!(result, Value::Boolean(false)); // They're equal!
}

// ============================================================================
// Strict Equality (===) Tests
// ============================================================================

#[test]
fn test_strict_eq_primitives() {
    let mut ctx = Context::new().unwrap();
    
    // Same primitives with same type
    let result = ctx.eval("5 === 5").unwrap();
    assert_eq!(result, Value::Boolean(true));
    
    let result = ctx.eval("5 === \"5\"").unwrap();
    assert_eq!(result, Value::Boolean(false)); // Different types!
    
    let result = ctx.eval("true === true").unwrap();
    assert_eq!(result, Value::Boolean(true));
}

#[test]
fn test_strict_eq_inequality() {
    let mut ctx = Context::new().unwrap();
    
    // !== should check strict inequality
    let result = ctx.eval("5 !== \"5\"").unwrap();
    assert_eq!(result, Value::Boolean(true)); // Different types!
}

// ============================================================================
// instanceof Tests
// ============================================================================
//
// Note: instanceof requires that the constructor (right operand) has a proper
// `prototype` property that points to an object. Currently, some built-in
// constructors like Array and Object are registered as plain Objects without
// the prototype chain set up correctly. Date works because it uses
// NativeConstructor. These tests document the current behavior.

#[test]
fn test_instanceof_date() {
    let mut ctx = Context::new().unwrap();
    
    // Date objects should be instances of Date
    // This works because Date is registered as a NativeConstructor
    let result = ctx.eval("new Date() instanceof Date").unwrap();
    assert_eq!(result, Value::Boolean(true));
}

#[test]
fn test_instanceof_primitives() {
    let mut ctx = Context::new().unwrap();
    
    // instanceof with non-object left side should return false
    let result = ctx.eval("123 instanceof Date").unwrap();
    assert_eq!(result, Value::Boolean(false));
    
    let result = ctx.eval("\"hello\" instanceof Date").unwrap();
    assert_eq!(result, Value::Boolean(false));
    
    let result = ctx.eval("null instanceof Date").unwrap();
    assert_eq!(result, Value::Boolean(false));
    
    let result = ctx.eval("undefined instanceof Date").unwrap();
    assert_eq!(result, Value::Boolean(false));
}

// ============================================================================
// in Operator Tests
// ============================================================================

#[test]
fn test_in_operator_object() {
    let mut ctx = Context::new().unwrap();
    
    // Property exists in object
    let result = ctx.eval("\"foo\" in { foo: 1 }").unwrap();
    assert_eq!(result, Value::Boolean(true));
    
    // Property does not exist in object
    let result = ctx.eval("\"bar\" in { foo: 1 }").unwrap();
    assert_eq!(result, Value::Boolean(false));
}

#[test]
fn test_in_operator_array() {
    let mut ctx = Context::new().unwrap();
    
    // Array index exists
    let result = ctx.eval("0 in [1, 2, 3]").unwrap();
    assert_eq!(result, Value::Boolean(true));
    
    let result = ctx.eval("2 in [1, 2, 3]").unwrap();
    assert_eq!(result, Value::Boolean(true));
    
    // Array index does not exist
    let result = ctx.eval("5 in [1, 2, 3]").unwrap();
    assert_eq!(result, Value::Boolean(false));
    
    // String index on array
    let result = ctx.eval("\"length\" in [1, 2, 3]").unwrap();
    assert_eq!(result, Value::Boolean(true));
}

#[test]
fn test_in_operator_string() {
    let mut ctx = Context::new().unwrap();
    
    // Character index exists in string
    let result = ctx.eval("0 in \"hello\"").unwrap();
    assert_eq!(result, Value::Boolean(true));
    
    let result = ctx.eval("4 in \"hello\"").unwrap();
    assert_eq!(result, Value::Boolean(true));
    
    // Character index does not exist
    let result = ctx.eval("10 in \"hello\"").unwrap();
    assert_eq!(result, Value::Boolean(false));
}

// ============================================================================
// Combined Tests
// ============================================================================

#[test]
fn test_eq_chain() {
    let mut ctx = Context::new().unwrap();
    
    // Chained comparisons - evaluated left to right
    // (1 < 2) = true, then (true < 3) coerces to (1 < 3) = true
    let result = ctx.eval("1 < 2 < 3").unwrap();
    assert_eq!(result, Value::Boolean(true));
    
    // (3 > 2) = true, then (true > 1) coerces to (1 > 1) = false
    let result = ctx.eval("3 > 2 > 1").unwrap();
    assert_eq!(result, Value::Boolean(false));
    
    // All comparisons
    let result = ctx.eval("1 < 2 < 3 < 4").unwrap();
    assert_eq!(result, Value::Boolean(true));
}

#[test]
fn test_complex_equality() {
    let mut ctx = Context::new().unwrap();
    
    // Complex equality expressions
    let result = ctx.eval("null == null").unwrap();
    assert_eq!(result, Value::Boolean(true));
    
    let result = ctx.eval("undefined == undefined").unwrap();
    assert_eq!(result, Value::Boolean(true));
    
    // Empty string coerces to 0
    let result = ctx.eval("\"\" == 0").unwrap();
    assert_eq!(result, Value::Boolean(true));
    
    // False == 0
    let result = ctx.eval("false == 0").unwrap();
    assert_eq!(result, Value::Boolean(true));
    
    // True == 1
    let result = ctx.eval("true == 1").unwrap();
    assert_eq!(result, Value::Boolean(true));
}
