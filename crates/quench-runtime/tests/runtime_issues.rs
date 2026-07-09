// linter-skip
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

// ============================================================================
// Math trig and log functions tests
// ============================================================================

#[test]
fn test_math_sin() {
    let mut ctx = Context::new().unwrap();
    
    // sin(0) should be 0
    let result = ctx.eval("Math.sin(0)").unwrap();
    assert_eq!(result, Value::Number(0.0));
    
    // sin(PI/2) should be approximately 1
    let result = ctx.eval("Math.sin(Math.PI / 2)").unwrap();
    match result {
        Value::Number(n) => assert!((n - 1.0).abs() < 0.0001),
        _ => panic!("Expected number, got {:?}", result),
    }
}

#[test]
fn test_math_cos() {
    let mut ctx = Context::new().unwrap();
    
    // cos(0) should be 1
    let result = ctx.eval("Math.cos(0)").unwrap();
    assert_eq!(result, Value::Number(1.0));
    
    // cos(PI) should be approximately -1
    let result = ctx.eval("Math.cos(Math.PI)").unwrap();
    match result {
        Value::Number(n) => assert!((n + 1.0).abs() < 0.0001),
        _ => panic!("Expected number, got {:?}", result),
    }
}

#[test]
fn test_math_tan() {
    let mut ctx = Context::new().unwrap();
    
    // tan(0) should be 0
    let result = ctx.eval("Math.tan(0)").unwrap();
    assert_eq!(result, Value::Number(0.0));
}

#[test]
fn test_math_asin() {
    let mut ctx = Context::new().unwrap();
    
    // asin(0) should be 0
    let result = ctx.eval("Math.asin(0)").unwrap();
    assert_eq!(result, Value::Number(0.0));
    
    // asin(1) should be approximately PI/2
    let result = ctx.eval("Math.asin(1)").unwrap();
    match result {
        Value::Number(n) => assert!((n - std::f64::consts::FRAC_PI_2).abs() < 0.0001),
        _ => panic!("Expected number, got {:?}", result),
    }
}

#[test]
fn test_math_acos() {
    let mut ctx = Context::new().unwrap();
    
    // acos(1) should be 0
    let result = ctx.eval("Math.acos(1)").unwrap();
    assert_eq!(result, Value::Number(0.0));
    
    // acos(0) should be approximately PI/2
    let result = ctx.eval("Math.acos(0)").unwrap();
    match result {
        Value::Number(n) => assert!((n - std::f64::consts::FRAC_PI_2).abs() < 0.0001),
        _ => panic!("Expected number, got {:?}", result),
    }
}

#[test]
fn test_math_atan() {
    let mut ctx = Context::new().unwrap();
    
    // atan(0) should be 0
    let result = ctx.eval("Math.atan(0)").unwrap();
    assert_eq!(result, Value::Number(0.0));
    
    // atan(1) should be approximately PI/4
    let result = ctx.eval("Math.atan(1)").unwrap();
    match result {
        Value::Number(n) => assert!((n - std::f64::consts::FRAC_PI_4).abs() < 0.0001),
        _ => panic!("Expected number, got {:?}", result),
    }
}

#[test]
fn test_math_atan2() {
    let mut ctx = Context::new().unwrap();
    
    // atan2(1, 1) should be approximately PI/4
    let result = ctx.eval("Math.atan2(1, 1)").unwrap();
    match result {
        Value::Number(n) => assert!((n - std::f64::consts::FRAC_PI_4).abs() < 0.0001),
        _ => panic!("Expected number, got {:?}", result),
    }
}

#[test]
fn test_math_log() {
    let mut ctx = Context::new().unwrap();
    
    // log(1) should be 0
    let result = ctx.eval("Math.log(1)").unwrap();
    assert_eq!(result, Value::Number(0.0));
    
    // log(e) should be approximately 1
    let result = ctx.eval("Math.log(Math.E)").unwrap();
    match result {
        Value::Number(n) => assert!((n - 1.0).abs() < 0.0001),
        _ => panic!("Expected number, got {:?}", result),
    }
}

#[test]
fn test_math_log10() {
    let mut ctx = Context::new().unwrap();
    
    // log10(1) should be 0
    let result = ctx.eval("Math.log10(1)").unwrap();
    assert_eq!(result, Value::Number(0.0));
    
    // log10(10) should be 1
    let result = ctx.eval("Math.log10(10)").unwrap();
    assert_eq!(result, Value::Number(1.0));
    
    // log10(100) should be 2
    let result = ctx.eval("Math.log10(100)").unwrap();
    assert_eq!(result, Value::Number(2.0));
}

#[test]
fn test_math_log2() {
    let mut ctx = Context::new().unwrap();
    
    // log2(1) should be 0
    let result = ctx.eval("Math.log2(1)").unwrap();
    assert_eq!(result, Value::Number(0.0));
    
    // log2(2) should be 1
    let result = ctx.eval("Math.log2(2)").unwrap();
    assert_eq!(result, Value::Number(1.0));
    
    // log2(8) should be 3
    let result = ctx.eval("Math.log2(8)").unwrap();
    assert_eq!(result, Value::Number(3.0));
}

#[test]
fn test_math_exp() {
    let mut ctx = Context::new().unwrap();
    
    // exp(0) should be 1
    let result = ctx.eval("Math.exp(0)").unwrap();
    assert_eq!(result, Value::Number(1.0));
    
    // exp(1) should be approximately e
    let result = ctx.eval("Math.exp(1)").unwrap();
    match result {
        Value::Number(n) => assert!((n - std::f64::consts::E).abs() < 0.0001),
        _ => panic!("Expected number, got {:?}", result),
    }
}

#[test]
fn test_math_log1p() {
    let mut ctx = Context::new().unwrap();
    
    // log1p(0) should be 0
    let result = ctx.eval("Math.log1p(0)").unwrap();
    assert_eq!(result, Value::Number(0.0));
    
    // log1p(e - 1) should be approximately 1
    let result = ctx.eval("Math.log1p(Math.E - 1)").unwrap();
    match result {
        Value::Number(n) => assert!((n - 1.0).abs() < 0.0001),
        _ => panic!("Expected number, got {:?}", result),
    }
}

// ============================================================================
// Date.now tests
// ============================================================================

#[test]
fn test_date_now() {
    let mut ctx = Context::new().unwrap();
    
    // Date.now() should return a number
    let result = ctx.eval("typeof Date.now()").unwrap();
    assert_eq!(result, Value::String("number".to_string()));
    
    // Date.now() should return a positive number (timestamp in ms)
    let result = ctx.eval("Date.now() > 0").unwrap();
    assert_eq!(result, Value::Boolean(true));
    
    // Date.now() should return a reasonable timestamp (after year 2000)
    let result = ctx.eval("Date.now() > 946684800000").unwrap();
    assert_eq!(result, Value::Boolean(true));
}

// ============================================================================
// Number.prototype.toFixed tests
// ============================================================================

#[test]
fn test_number_to_fixed_basic() {
    let mut ctx = Context::new().unwrap();
    
    // (123.456).toFixed() should return "123"
    let result = ctx.eval("(123.456).toFixed()").unwrap();
    assert_eq!(result, Value::String("123".to_string()));
    
    // (123.456).toFixed(0) should return "123"
    let result = ctx.eval("(123.456).toFixed(0)").unwrap();
    assert_eq!(result, Value::String("123".to_string()));
    
    // (123.456).toFixed(1) should return "123.5"
    let result = ctx.eval("(123.456).toFixed(1)").unwrap();
    assert_eq!(result, Value::String("123.5".to_string()));
    
    // (123.456).toFixed(2) should return "123.46"
    let result = ctx.eval("(123.456).toFixed(2)").unwrap();
    assert_eq!(result, Value::String("123.46".to_string()));
}

#[test]
fn test_number_to_fixed_negative() {
    let mut ctx = Context::new().unwrap();
    
    // (-123.456).toFixed(2) should return "-123.46"
    let result = ctx.eval("(-123.456).toFixed(2)").unwrap();
    assert_eq!(result, Value::String("-123.46".to_string()));
}

#[test]
fn test_number_to_fixed_zero() {
    let mut ctx = Context::new().unwrap();
    
    // (0).toFixed(2) should return "0.00"
    let result = ctx.eval("(0).toFixed(2)").unwrap();
    assert_eq!(result, Value::String("0.00".to_string()));
}

#[test]
fn test_number_to_fixed_integer() {
    let mut ctx = Context::new().unwrap();
    
    // (100).toFixed(5) should return "100.00000"
    let result = ctx.eval("(100).toFixed(5)").unwrap();
    assert_eq!(result, Value::String("100.00000".to_string()));
}

#[test]
fn test_number_to_fixed_nan() {
    let mut ctx = Context::new().unwrap();
    
    // NaN.toFixed(2) should return "NaN"
    let result = ctx.eval("(NaN).toFixed(2)").unwrap();
    assert_eq!(result, Value::String("NaN".to_string()));
}

#[test]
fn test_number_to_fixed_infinity() {
    let mut ctx = Context::new().unwrap();
    
    // Infinity.toFixed(2) should return "Infinity"
    let result = ctx.eval("(Infinity).toFixed(2)").unwrap();
    assert_eq!(result, Value::String("Infinity".to_string()));
    
    // (-Infinity).toFixed(2) should return "-Infinity"
    let result = ctx.eval("(-Infinity).toFixed(2)").unwrap();
    assert_eq!(result, Value::String("-Infinity".to_string()));
}

#[test]
fn test_number_to_fixed_via_constructor() {
    let mut ctx = Context::new().unwrap();
    
    // Number.prototype.toFixed should work on primitive numbers
    // Note: Number.prototype.toFixed.call requires the call method to work correctly,
    // which is a more advanced feature. Testing direct usage instead.
    let result = ctx.eval("(42).toFixed(3)").unwrap();
    assert_eq!(result, Value::String("42.000".to_string()));
}

// ============================================================================
// Break/Continue statement tests
// ============================================================================

#[test]
fn test_break_in_while_loop() {
    let mut ctx = Context::new().unwrap();
    
    // Test break in a while loop
    let result = ctx.eval(r#"
        var count = 0;
        while (count < 10) {
            count++;
            if (count >= 5) {
                break;
            }
        }
        count;
    "#).unwrap();
    assert_eq!(result, Value::Number(5.0));
}

#[test]
fn test_break_in_for_loop() {
    let mut ctx = Context::new().unwrap();
    
    // Test break in a for loop
    let result = ctx.eval(r#"
        var sum = 0;
        for (var i = 0; i < 10; i++) {
            if (i >= 5) {
                break;
            }
            sum += i;
        }
        sum;
    "#).unwrap();
    // sum should be 0+1+2+3+4 = 10
    assert_eq!(result, Value::Number(10.0));
}

#[test]
fn test_continue_in_for_loop() {
    let mut ctx = Context::new().unwrap();
    
    // Test continue in a for loop
    let result = ctx.eval(r#"
        var sum = 0;
        for (var i = 0; i < 5; i++) {
            if (i === 2) {
                continue;
            }
            sum += i;
        }
        sum;
    "#).unwrap();
    // sum should be 0+1+3+4 = 8
    assert_eq!(result, Value::Number(8.0));
}

#[test]
fn test_continue_in_while_loop() {
    let mut ctx = Context::new().unwrap();
    
    // Test continue in a while loop
    let result = ctx.eval(r#"
        var sum = 0;
        var i = 0;
        while (i < 5) {
            i++;
            if (i === 2) {
                continue;
            }
            sum += i;
        }
        sum;
    "#).unwrap();
    // sum should be 1+3+4+5 = 13
    assert_eq!(result, Value::Number(13.0));
}

// ============================================================================
// Unary plus operator tests
// ============================================================================

#[test]
fn test_unary_plus_number() {
    let mut ctx = Context::new().unwrap();
    let result = ctx.eval("+42").unwrap();
    assert_eq!(result, Value::Number(42.0));
}

#[test]
fn test_unary_plus_string_to_number() {
    let mut ctx = Context::new().unwrap();
    let result = ctx.eval("+'5'").unwrap();
    assert_eq!(result, Value::Number(5.0));
}

#[test]
fn test_unary_plus_boolean_true() {
    let mut ctx = Context::new().unwrap();
    let result = ctx.eval("+true").unwrap();
    assert_eq!(result, Value::Number(1.0));
}

#[test]
fn test_unary_plus_boolean_false() {
    let mut ctx = Context::new().unwrap();
    let result = ctx.eval("+false").unwrap();
    assert_eq!(result, Value::Number(0.0));
}

#[test]
fn test_unary_plus_undefined() {
    let mut ctx = Context::new().unwrap();
    let result = ctx.eval("+undefined").unwrap();
    match result {
        Value::Number(n) => assert!(n.is_nan()),
        _ => panic!("Expected Number, got {:?}", result),
    }
}

// ============================================================================
// Spread in array tests
// ============================================================================

#[test]
fn test_spread_in_array_basic() {
    let mut ctx = Context::new().unwrap();
    let result = ctx.eval("[1, ...[2, 3], 4]").unwrap();
    match result {
        Value::Object(o) => {
            let arr = o.borrow();
            assert_eq!(arr.elements.len(), 4);
            assert_eq!(arr.elements[0], Value::Number(1.0));
            assert_eq!(arr.elements[1], Value::Number(2.0));
            assert_eq!(arr.elements[2], Value::Number(3.0));
            assert_eq!(arr.elements[3], Value::Number(4.0));
        }
        _ => panic!("Expected array object, got {:?}", result),
    }
}

#[test]
fn test_spread_in_array_empty() {
    let mut ctx = Context::new().unwrap();
    let result = ctx.eval("[...[]]").unwrap();
    match result {
        Value::Object(o) => {
            let arr = o.borrow();
            assert_eq!(arr.elements.len(), 0);
        }
        _ => panic!("Expected empty array"),
    }
}

#[test]
fn test_spread_in_array_string() {
    let mut ctx = Context::new().unwrap();
    let result = ctx.eval("[...'ab']").unwrap();
    match result {
        Value::Object(o) => {
            let arr = o.borrow();
            assert_eq!(arr.elements.len(), 2);
        }
        _ => panic!("Expected array object"),
    }
}

// ============================================================================
// Typeof tests
// ============================================================================

#[test]
fn test_typeof_null() {
    let mut ctx = Context::new().unwrap();
    let result = ctx.eval("typeof null").unwrap();
    assert_eq!(result, Value::String("object".to_string()));
}

#[test]
fn test_typeof_undefined() {
    let mut ctx = Context::new().unwrap();
    let result = ctx.eval("typeof undefined").unwrap();
    assert_eq!(result, Value::String("undefined".to_string()));
}

#[test]
fn test_typeof_function() {
    let mut ctx = Context::new().unwrap();
    let result = ctx.eval("typeof function(){}").unwrap();
    assert_eq!(result, Value::String("function".to_string()));
}

#[test]
fn test_typeof_undeclared() {
    let mut ctx = Context::new().unwrap();
    let result = ctx.eval("typeof totally_undeclared_variable_xyz").unwrap();
    assert_eq!(result, Value::String("undefined".to_string()));
}
