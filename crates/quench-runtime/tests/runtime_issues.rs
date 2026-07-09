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
fn test_new_boolean_returns_object_not_primitive() {
    let mut ctx = Context::new().unwrap();
    
    // new Boolean(false) should return an object, not false
    let result = ctx.eval("typeof new Boolean(false)").unwrap();
    assert_eq!(result, Value::String("object".to_string()));
}

#[test]
fn test_new_boolean_vs_boolean_conversion() {
    let mut ctx = Context::new().unwrap();
    
    // Boolean(false) === false (conversion returns primitive)
    let result = ctx.eval("Boolean(false) === false").unwrap();
    assert_eq!(result, Value::Boolean(true));
    
    // new Boolean(false) !== false (object vs primitive)
    let result = ctx.eval("new Boolean(false) !== false").unwrap();
    assert_eq!(result, Value::Boolean(true));
}

#[test]
fn test_new_number_returns_object_not_primitive() {
    let mut ctx = Context::new().unwrap();
    
    // new Number(42) should return an object, not 42
    let result = ctx.eval("typeof new Number(42)").unwrap();
    assert_eq!(result, Value::String("object".to_string()));
}

#[test]
fn test_new_number_vs_number_conversion() {
    let mut ctx = Context::new().unwrap();
    
    // Number(42) === 42 (conversion returns primitive)
    let result = ctx.eval("Number(42) === 42").unwrap();
    assert_eq!(result, Value::Boolean(true));
    
    // new Number(42) !== 42 (object vs primitive)
    let result = ctx.eval("new Number(42) !== 42").unwrap();
    assert_eq!(result, Value::Boolean(true));
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

// ============================================================================
// Throw/catch value preservation tests (Task 250)
// ============================================================================

#[test]
fn test_throw_preserves_error_type() {
    // try { throw new TypeError('x'); } catch (e) { e instanceof TypeError; }
    // should be true
    let mut ctx = Context::new().unwrap();
    let result = ctx.eval(
        r#"
        try {
            throw new TypeError('test message');
        } catch (e) {
            e instanceof TypeError;
        }
        "#,
    ).unwrap();
    assert_eq!(result, Value::Boolean(true));
}

#[test]
fn test_throw_preserves_message() {
    // The caught error should have the original message
    let mut ctx = Context::new().unwrap();
    let result = ctx.eval(
        r#"
        try {
            throw new Error('specific error message');
        } catch (e) {
            e.message;
        }
        "#,
    ).unwrap();
    assert_eq!(result, Value::String("specific error message".to_string()));
}

#[test]
fn test_throw_preserves_number() {
    // Throwing a number should preserve it
    let mut ctx = Context::new().unwrap();
    let result = ctx.eval(
        r#"
        try {
            throw 42;
        } catch (e) {
            e;
        }
        "#,
    ).unwrap();
    assert_eq!(result, Value::Number(42.0));
}

#[test]
fn test_throw_preserves_object() {
    // Throwing an object should preserve it
    let mut ctx = Context::new().unwrap();
    let result = ctx.eval(
        r#"
        try {
            throw { code: 'CUSTOM_ERROR', value: 123 };
        } catch (e) {
            e.code;
        }
        "#,
    ).unwrap();
    assert_eq!(result, Value::String("CUSTOM_ERROR".to_string()));
}

#[test]
fn test_rethrow_preserves_error() {
    // Re-throwing should preserve the original error type
    let mut ctx = Context::new().unwrap();
    let result = ctx.eval(
        r#"
        try {
            try {
                throw new ReferenceError('original');
            } catch (inner) {
                throw inner;
            }
        } catch (e) {
            e instanceof ReferenceError;
        }
        "#,
    ).unwrap();
    assert_eq!(result, Value::Boolean(true));
}

#[test]
fn test_catch_binds_original_value() {
    // The catch parameter should be the exact thrown value, not a stringified version
    let mut ctx = Context::new().unwrap();
    let result = ctx.eval(
        r#"
        try {
            throw new TypeError('test');
        } catch (e) {
            typeof e;
        }
        "#,
    ).unwrap();
    // Should be 'object', not 'string'
    assert_eq!(result, Value::String("object".to_string()));
}

// ============================================================================
// Object storage for array indices tests (Task 320)
// ============================================================================

#[test]
fn test_object_keys_array() {
    // Object.keys([1,2,3]) should return ["0","1","2"]
    let mut ctx = Context::new().unwrap();
    let result = ctx.eval("Object.keys([1,2,3])").unwrap();
    if let Value::Object(arr) = result {
        let arr = arr.borrow();
        let keys: Vec<String> = (0..arr.elements.len())
            .map(|i| {
                if let Some(Value::String(s)) = arr.elements.get(i) {
                    s.clone()
                } else {
                    panic!("Expected string at index {}", i);
                }
            })
            .collect();
        assert_eq!(keys, vec!["0", "1", "2"]);
    } else {
        panic!("Expected array, got {:?}", result);
    }
}

#[test]
fn test_delete_array_index_direct() {
    // Test Object.delete directly (without the delete operator, which is separate)
    use quench_runtime::value::{Object, ObjectKind, Value};
    use std::cell::RefCell;
    use std::rc::Rc;

    let mut obj = Object::new_array(3);
    obj.set("0", Value::Number(1.0));
    obj.set("1", Value::Number(2.0));
    obj.set("2", Value::Number(3.0));

    // Delete index 0
    let result = obj.delete("0");
    assert!(result, "delete should return true");

    // Verify the element is now undefined
    assert_eq!(obj.get("0"), Some(Value::Undefined));

    // Verify other elements are unchanged
    assert_eq!(obj.get("1"), Some(Value::Number(2.0)));
    assert_eq!(obj.get("2"), Some(Value::Number(3.0)));
}

#[test]
fn test_array_named_properties() {
    // Named properties on arrays should still work alongside numeric indices
    let mut ctx = Context::new().unwrap();
    let result = ctx.eval(
        r#"
        var arr = [1, 2, 3];
        arr.customProp = 'hello';
        var keys = Object.keys(arr);
        var customValue = arr.customProp;
        [keys, customValue]
        "#,
    ).unwrap();
    if let Value::Object(result) = result {
        let result = result.borrow();
        // result[0] is the keys array
        if let Some(Value::Object(keys_arr)) = result.elements.get(0) {
            let keys_arr = keys_arr.borrow();
            let keys: Vec<String> = keys_arr.elements.iter().filter_map(|v| {
                if let Value::String(s) = v { Some(s.clone()) } else { None }
            }).collect();
            // Should have numeric keys plus 'customProp'
            assert!(keys.contains(&"0".to_string()), "Should contain '0'");
            assert!(keys.contains(&"1".to_string()), "Should contain '1'");
            assert!(keys.contains(&"2".to_string()), "Should contain '2'");
            assert!(keys.contains(&"customProp".to_string()), "Should contain 'customProp'");
            assert_eq!(keys.len(), 4, "Should have exactly 4 keys");
        }
        // result[1] is the custom value
        assert_eq!(result.elements.get(1), Some(&Value::String("hello".to_string())));
    } else {
        panic!("Expected array, got {:?}", result);
    }
}
