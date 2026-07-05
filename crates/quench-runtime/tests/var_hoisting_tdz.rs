//! Tests for var hoisting and let/const TDZ
//!
//! These tests verify the acceptance criteria:
//! - console.log(x); var x = 1; logs undefined
//! - x; let x = 1; throws ReferenceError (TDZ)
//! - const x = 1; x = 2; throws TypeError

use serial_test::serial;
use quench_runtime::Context;
use quench_runtime::Value;
use quench_runtime::interpreter::reset_depth;

/// Acceptance criterion 1: var x = 1; should hoist x with undefined value
#[serial]
#[test]
fn test_var_hoisting_basic() {
    reset_depth();
    let mut ctx = Context::new().unwrap();
    
    // var x; should be hoisted and accessible before declaration
    let result = ctx.eval(r#"
        var result = typeof x;
        var x = 5;
        result;
    "#).unwrap();
    
    assert_eq!(result, Value::String("undefined".to_string()), "var should be hoisted to undefined");
}

/// Acceptance criterion 1: var should be accessible before its declaration
#[serial]
#[test]
fn test_var_hoisting_logs_undefined() {
    reset_depth();
    let mut ctx = Context::new().unwrap();
    
    // var x = 1; should hoist x with undefined value before initialization
    let result = ctx.eval(r#"
        var output;
        function test() {
            output = x;
            var x = 1;
        }
        test();
        output;
    "#).unwrap();
    
    assert_eq!(result, Value::Undefined, "var should be hoisted and initialized to undefined before assignment");
}

/// Acceptance criterion 2: Accessing let before initialization should throw ReferenceError (TDZ)
#[serial]
#[test]
fn test_let_tdz_with_reference_error() {
    reset_depth();
    let mut ctx = Context::new().unwrap();
    
    // x; let x = 1; should throw ReferenceError (TDZ)
    let result = ctx.eval(r#"
        let x = x + 1;
    "#);
    
    assert!(result.is_err(), "Using let before initialization should throw");
    let err = result.unwrap_err();
    assert!(err.to_string().contains("Cannot access 'x' before initialization"),
            "Error should mention TDZ, got: {}", err.to_string());
}

/// Acceptance criterion 2: let should be accessible after initialization
#[serial]
#[test]
fn test_let_no_tdz_after_init() {
    reset_depth();
    let mut ctx = Context::new().unwrap();
    
    // After initialization, let should be accessible
    let result = ctx.eval(r#"
        let x = 5;
        x;
    "#).unwrap();
    
    assert_eq!(result, Value::Number(5.0));
}

/// Acceptance criterion 2: TDZ access should throw ReferenceError
#[serial]
#[test]
fn test_let_tdz_access_before_init() {
    reset_depth();
    let mut ctx = Context::new().unwrap();
    
    // Accessing let before initialization should throw ReferenceError
    let result = ctx.eval(r#"
        let x = y + 1;
        let y = 10;
        x;
    "#);
    
    // This should fail because y is in TDZ when x is initialized
    assert!(result.is_err(), "Accessing let before initialization should throw");
    let err = result.unwrap_err();
    assert!(err.to_string().contains("Cannot access 'y' before initialization"),
            "Error should mention TDZ, got: {}", err.to_string());
}

/// Acceptance criterion 2: const TDZ
#[serial]
#[test]
fn test_const_tdz_access_before_init() {
    reset_depth();
    let mut ctx = Context::new().unwrap();
    
    // Accessing const before initialization should throw ReferenceError (TDZ)
    let result = ctx.eval(r#"
        const x = x + 1;
    "#);
    
    assert!(result.is_err(), "Accessing const before initialization should throw");
    let err = result.unwrap_err();
    assert!(err.to_string().contains("Cannot access 'x' before initialization"),
            "Error should mention TDZ, got: {}", err.to_string());
}

/// Acceptance criterion 3: const assignment throws TypeError
#[serial]
#[test]
fn test_const_assignment_throws_type_error() {
    reset_depth();
    let mut ctx = Context::new().unwrap();
    
    // const x = 1; x = 2; should throw TypeError
    let result = ctx.eval(r#"
        function test() {
            const x = 1;
            x = 2;
        }
        test();
    "#);
    
    assert!(result.is_err(), "Assignment to const should throw");
    let err = result.unwrap_err();
    assert!(err.to_string().contains("Assignment to constant variable"),
            "Error should mention assignment to constant, got: {}", err.to_string());
}

/// Acceptance criterion 3: const can be read after initialization
#[serial]
#[test]
fn test_const_can_read_after_init() {
    reset_depth();
    let mut ctx = Context::new().unwrap();
    
    // const should be readable after initialization
    let result = ctx.eval(r#"
        const x = 42;
        x;
    "#).unwrap();
    
    assert_eq!(result, Value::Number(42.0));
}

/// Additional test: var is function-scoped
#[serial]
#[test]
fn test_var_function_scope() {
    reset_depth();
    let mut ctx = Context::new().unwrap();
    
    // var should be function-scoped, not block-scoped
    let result = ctx.eval(r#"
        function test() {
            if (true) {
                var x = 1;
            }
            return x;
        }
        test();
    "#).unwrap();
    
    assert_eq!(result, Value::Number(1.0), "var should be accessible outside block");
}

/// Additional test: var hoisting in for loop
#[serial]
#[test]
fn test_for_loop_var_hoisting() {
    reset_depth();
    let mut ctx = Context::new().unwrap();
    
    // var in for loop should be hoisted to function scope
    let result = ctx.eval(r#"
        function test() {
            var i;
            for (i = 0; i < 3; i++) {
                // loop body
            }
            return i;
        }
        test();
    "#).unwrap();
    
    assert_eq!(result, Value::Number(3.0), "var should be accessible after loop");
}

/// Additional test: nested TDZ scenario
#[serial]
#[test]
fn test_nested_tdz() {
    reset_depth();
    let mut ctx = Context::new().unwrap();
    
    // TDZ should work with nested references
    let result = ctx.eval(r#"
        let a = b + 1;
        let b = 10;
        a;
    "#);
    
    // This should fail because b is accessed before initialization
    assert!(result.is_err(), "TDZ should catch nested references");
}

/// Test TDZ with shadowing - inner let should shadow outer let
/// This is the key bug: accessing `x` before the inner `let x = 2` should throw TDZ
#[serial]
#[test]
fn test_tdz_shadowing_inner_let() {
    reset_depth();
    let mut ctx = Context::new().unwrap();
    
    // Inner let shadows outer let, accessing x before initialization should throw
    let result = ctx.eval(r#"
        let x = 1;
        function outer() {
            x; // This should throw TDZ because inner x shadows outer x
            let x = 2;
        }
        outer();
    "#);
    
    assert!(result.is_err(), "TDZ should throw when inner let shadows outer and is accessed before init");
    let err = result.unwrap_err();
    assert!(err.to_string().contains("Cannot access 'x' before initialization"),
            "Error should mention TDZ for x, got: {}", err.to_string());
}

/// Test that after the inner let is initialized, it can be accessed
#[serial]
#[test]
fn test_tdz_shadowing_after_init() {
    reset_depth();
    let mut ctx = Context::new().unwrap();
    
    let result = ctx.eval(r#"
        let x = 1;
        function outer() {
            let x = 2;
            return x; // This should return 2, not 1
        }
        outer();
    "#).unwrap();
    
    assert_eq!(result, Value::Number(2.0), "Inner let should shadow outer let");
}

/// Test TDZ in block scope
#[serial]
#[test]
fn test_tdz_block_scope() {
    reset_depth();
    let mut ctx = Context::new().unwrap();
    
    let result = ctx.eval(r#"
        let x = 1;
        {
            x; // This should throw TDZ because inner block has let x = 2
            let x = 2;
        }
    "#);
    
    assert!(result.is_err(), "TDZ should throw when block-scoped let shadows outer");
}

/// Test that typeof this returns "object" not "undefined"
/// This was a bug where typeof "this" was incorrectly returning "undefined"
/// because the typeof special handling was checking if "this" was in the environment.
#[serial]
#[test]
fn test_typeof_this_returns_object() {
    reset_depth();
    let mut ctx = Context::new().unwrap();
    
    let result = ctx.eval(r#"
        typeof this;
    "#).unwrap();
    
    assert_eq!(result, Value::String("object".to_string()), "typeof this should return 'object'");
}

/// Test that constructor with expression statement returns correct value
/// This was a bug where `this.x = value;` was causing the constructor
/// to return the value instead of the implicit this.
#[serial]
#[test]
fn test_constructor_returns_this_not_expression_value() {
    reset_depth();
    let mut ctx = Context::new().unwrap();
    
    let result = ctx.eval(r#"
        function Test(props) {
            this.props = props || {};
        }
        var t = new Test(undefined);
        t.props !== undefined;
    "#).unwrap();
    
    assert_eq!(result, Value::Boolean(true), "constructor should return this, not expression value");
}

/// Test that eval returns the last expression value
#[serial]
#[test]
fn test_eval_returns_expression_value() {
    reset_depth();
    let mut ctx = Context::new().unwrap();
    
    let result = ctx.eval(r#"
        1 + 2;
    "#).unwrap();
    
    assert_eq!(result, Value::Number(3.0), "eval should return last expression value");
}
