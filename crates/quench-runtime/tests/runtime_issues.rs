//! Runtime issue regression tests
//!
//! Tests for issues identified in Task 58.

#![allow(unknown_lints, clippy::function_length)]

use quench_runtime::Context;
use quench_runtime::Value;

#[test]
fn test_date_has_object_prototype() {
    let mut ctx = Context::new().unwrap();
    
    // Date should inherit from Object.prototype
    let result = ctx.eval("new Date()").unwrap();
    match &result {
        Value::Object(obj) => {
            let obj = obj.borrow();
            // Should be able to call Object.prototype methods on Date
            assert!(obj.has("toString"), "Date should have toString");
            assert!(obj.has("valueOf"), "Date should have valueOf");
        }
        _ => panic!("Expected Date object, got {:?}", result),
    }
}

#[test]
fn test_error_has_object_prototype() {
    let mut ctx = Context::new().unwrap();
    
    // Error should inherit from Object.prototype
    let result = ctx.eval("new Error()").unwrap();
    match &result {
        Value::Object(obj) => {
            let obj = obj.borrow();
            assert!(obj.has("toString"), "Error should have toString");
            assert!(obj.has("message"), "Error should have message property");
        }
        _ => panic!("Expected Error object, got {:?}", result),
    }
}

#[test]
fn test_type_error_has_object_prototype() {
    let mut ctx = Context::new().unwrap();
    
    // TypeError should inherit from Error.prototype which inherits from Object.prototype
    let result = ctx.eval("new TypeError()").unwrap();
    match &result {
        Value::Object(obj) => {
            let obj = obj.borrow();
            assert!(obj.has("toString"), "TypeError should have toString");
        }
        _ => panic!("Expected TypeError object, got {:?}", result),
    }
}

#[test]
fn test_set_timeout_returns_number() {
    let mut ctx = Context::new().unwrap();
    
    // setTimeout should return a number (timer ID)
    let result = ctx.eval("typeof setTimeout(() => {}, 100)").unwrap();
    assert_eq!(result, Value::String("number".to_string()));
}

#[test]
fn test_set_interval_returns_number() {
    let mut ctx = Context::new().unwrap();
    
    // setInterval should return a number (timer ID)
    let result = ctx.eval("typeof setInterval(() => {}, 100)").unwrap();
    assert_eq!(result, Value::String("number".to_string()));
}

#[test]
fn test_clear_timeout_works() {
    let mut ctx = Context::new().unwrap();
    
    // clearTimeout should not throw
    let result = ctx.eval("clearTimeout(1); clearTimeout(0)").unwrap();
    assert_eq!(result, Value::Undefined);
}

#[test]
fn test_clear_interval_works() {
    let mut ctx = Context::new().unwrap();
    
    // clearInterval should not throw
    let result = ctx.eval("clearInterval(1); clearInterval(0)").unwrap();
    assert_eq!(result, Value::Undefined);
}

#[test]
fn test_number_constructor_has_prototype() {
    let mut ctx = Context::new().unwrap();
    
    // Number constructor should have a prototype property
    let result = ctx.eval("typeof Number.prototype").unwrap();
    assert_eq!(result, Value::String("object".to_string()));
}

#[test]
fn test_string_constructor_has_prototype() {
    let mut ctx = Context::new().unwrap();
    
    // String constructor should have a prototype property
    let result = ctx.eval("typeof String.prototype").unwrap();
    assert_eq!(result, Value::String("object".to_string()));
}

#[test]
fn test_boolean_constructor_has_prototype() {
    let mut ctx = Context::new().unwrap();
    
    // Boolean constructor should have a prototype property
    let result = ctx.eval("typeof Boolean.prototype").unwrap();
    assert_eq!(result, Value::String("object".to_string()));
}

#[test]
fn test_function_constructor_has_prototype() {
    let mut ctx = Context::new().unwrap();
    
    // Function constructor should have a prototype property
    let result = ctx.eval("typeof Function.prototype").unwrap();
    assert_eq!(result, Value::String("object".to_string()));
}

#[test]
fn test_object_constructor_has_prototype() {
    let mut ctx = Context::new().unwrap();
    
    // Object constructor should have a prototype property
    let result = ctx.eval("typeof Object.prototype").unwrap();
    assert_eq!(result, Value::String("object".to_string()));
}

#[test]
fn test_array_constructor_has_prototype() {
    let mut ctx = Context::new().unwrap();
    
    // Array constructor should have a prototype property
    let result = ctx.eval("typeof Array.prototype").unwrap();
    assert_eq!(result, Value::String("object".to_string()));
}

#[test]
fn test_map_constructor_has_prototype() {
    let mut ctx = Context::new().unwrap();
    
    // Map constructor should have a prototype property
    let result = ctx.eval("typeof Map.prototype").unwrap();
    assert_eq!(result, Value::String("object".to_string()));
}

#[test]
fn test_set_constructor_has_prototype() {
    let mut ctx = Context::new().unwrap();
    
    // Set constructor should have a prototype property
    let result = ctx.eval("typeof Set.prototype").unwrap();
    assert_eq!(result, Value::String("object".to_string()));
}

#[test]
fn test_error_constructor_has_prototype() {
    let mut ctx = Context::new().unwrap();
    
    // Error constructor should have a prototype property
    let result = ctx.eval("typeof Error.prototype").unwrap();
    assert_eq!(result, Value::String("object".to_string()));
}

#[test]
fn test_type_error_constructor_has_prototype() {
    let mut ctx = Context::new().unwrap();
    
    // TypeError constructor should have a prototype property
    let result = ctx.eval("typeof TypeError.prototype").unwrap();
    assert_eq!(result, Value::String("object".to_string()));
}

#[test]
fn test_reference_error_constructor_has_prototype() {
    let mut ctx = Context::new().unwrap();
    
    // ReferenceError constructor should have a prototype property
    let result = ctx.eval("typeof ReferenceError.prototype").unwrap();
    assert_eq!(result, Value::String("object".to_string()));
}

#[test]
fn test_syntax_error_constructor_has_prototype() {
    let mut ctx = Context::new().unwrap();
    
    // SyntaxError constructor should have a prototype property
    let result = ctx.eval("typeof SyntaxError.prototype").unwrap();
    assert_eq!(result, Value::String("object".to_string()));
}

#[test]
fn test_date_constructor_has_prototype() {
    let mut ctx = Context::new().unwrap();
    
    // Date constructor should have a prototype property
    let result = ctx.eval("typeof Date.prototype").unwrap();
    assert_eq!(result, Value::String("object".to_string()));
}
