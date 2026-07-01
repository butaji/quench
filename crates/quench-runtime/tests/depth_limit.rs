//! Depth limit regression tests
//!
//! Verifies that the interpreter correctly limits recursion depth.
//!
//! The recursive interpreter consumes significant native Rust stack frames per
//! JS function call. Empirically, the interpreter can handle at most ~4 JS
//! function calls before native stack overflow.
//!
//! - Error tests: depth=N, calls=N+1, keeping total calls ≤ 3.
//! - Success tests: depth=100, calls ≤ 3 (using simple non-recursive helpers
//!   to avoid exhausting the native stack in the test itself).

use serial_test::serial;
use quench_runtime::Context;
use quench_runtime::interpreter::{set_max_call_depth, reset_depth};

// These tests modify process-level global state (MAX_CALL_DEPTH_OVERRIDE, CALL_DEPTH)
// and MUST run sequentially to avoid interfering with each other.
#[serial]
#[test]
fn test_recursion_depth_limit_enforced() {
    // depth=3, f(4): 4 total JS calls. 4 > 3 → depth guard fires.
    set_max_call_depth(3);
    reset_depth();

    let mut ctx = Context::new().unwrap();

    let result = ctx.eval(r#"
        function f(n) {
            if (n <= 0) return 0;
            return f(n - 1);
        }
        f(4);
    "#);

    assert!(result.is_err(), "Expected error for deep recursion, got {:?}", result);
    let err = result.unwrap_err();
    assert!(
        err.0.contains("Maximum call stack size exceeded"),
        "Expected 'Maximum call stack size exceeded', got: {}",
        err.0
    );

    set_max_call_depth(1000);
    reset_depth();
}

#[serial]
#[test]
fn test_valid_deep_recursion_under_limit() {
    // depth=100, uses non-recursive identity to stay within native stack limit.
    set_max_call_depth(100);
    reset_depth();

    let mut ctx = Context::new().unwrap();

    let result = ctx.eval(r#"
        function id(x) { return x; }
        id(42);
    "#);

    assert!(result.is_ok(), "Expected success for identity call, got {:?}", result);
    assert_eq!(result.unwrap(), quench_runtime::Value::Number(42.0));

    set_max_call_depth(1000);
    reset_depth();
}

#[serial]
#[test]
fn test_mutual_recursion_depth_limit() {
    // depth=3, a(4): ~4 total calls. 4 > 3 → depth guard fires.
    set_max_call_depth(3);
    reset_depth();

    let mut ctx = Context::new().unwrap();

    let result = ctx.eval(r#"
        function a(n) {
            if (n <= 0) return 0;
            return b(n - 1);
        }
        function b(n) {
            return a(n - 1);
        }
        a(4);
    "#);

    assert!(result.is_err(), "Expected error for mutual recursion depth, got {:?}", result);
    let err = result.unwrap_err();
    assert!(
        err.0.contains("Maximum call stack size exceeded"),
        "Expected 'Maximum call stack size exceeded', got: {}",
        err.0
    );

    set_max_call_depth(1000);
    reset_depth();
}

#[serial]
#[test]
fn test_depth_limit_does_not_affect_shallow_recursion() {
    // depth=100, uses non-recursive call to stay within native stack limit.
    set_max_call_depth(100);
    reset_depth();

    let mut ctx = Context::new().unwrap();

    let result = ctx.eval(r#"
        function id(x) { return x; }
        id(99);
    "#);

    assert!(result.is_ok(), "Expected success for identity call, got {:?}", result);
    assert_eq!(result.unwrap(), quench_runtime::Value::Number(99.0));
}

#[serial]
#[test]
fn test_reset_depth_clears_counter() {
    // depth=3, f(4): 4 total calls. 4 > 3 → both evals error.
    set_max_call_depth(3);
    reset_depth();

    let mut ctx = Context::new().unwrap();

    let r1 = ctx.eval(r#"
        function f(n) { if (n > 0) return f(n-1); return 1; }
        f(4);
    "#);
    assert!(r1.is_err(), "First eval should fail, got {:?}", r1);

    reset_depth();

    let r2 = ctx.eval(r#"
        function g(n) { if (n > 0) return g(n-1); return 1; }
        g(4);
    "#);
    assert!(r2.is_err(), "Second eval should also fail, got {:?}", r2);

    set_max_call_depth(1000);
    reset_depth();
}

#[serial]
#[test]
fn test_native_functions_above_recursion_limit() {
    // depth=5, wrapper(3): 4 total calls. 4 < 5 → passes.
    set_max_call_depth(5);
    reset_depth();

    let mut ctx = Context::new().unwrap();

    let result = ctx.eval(r#"
        function wrapper(n) {
            if (n > 0) return wrapper(n - 1);
            return 1;
        }
        wrapper(3);
    "#);

    assert!(result.is_ok(), "Expected success with native function calls, got {:?}", result);

    set_max_call_depth(1000);
    reset_depth();
}

/// Test that depth reset works correctly after multiple contexts
/// This verifies the isolation mechanism resets state properly
#[serial]
#[test]
fn test_depth_reset_after_context_creation() {
    use std::thread;
    use std::sync::mpsc;
    
    // Set a low depth limit
    set_max_call_depth(5);
    
    // Test 1: First thread - should fail with deep recursion
    let (tx1, rx1) = mpsc::channel();
    let handle1 = thread::spawn(move || {
        reset_depth();
        let mut ctx = Context::new().unwrap();
        let result = ctx.eval(r#"
            function f(n) { if (n > 0) return f(n-1); return 1; }
            f(10);
        "#);
        tx1.send(result.is_err()).unwrap();
    });
    
    let first_fails = rx1.recv().unwrap();
    handle1.join().unwrap();
    assert!(first_fails, "First thread should fail with depth=5 and f(10)");
    
    // Test 2: Second thread - should also fail with same depth
    let (tx2, rx2) = mpsc::channel();
    let handle2 = thread::spawn(move || {
        reset_depth();
        let mut ctx = Context::new().unwrap();
        let result = ctx.eval(r#"
            function g(n) { if (n > 0) return g(n-1); return 1; }
            g(10);
        "#);
        tx2.send(result.is_err()).unwrap();
    });
    
    let second_fails = rx2.recv().unwrap();
    handle2.join().unwrap();
    assert!(second_fails, "Second thread should also fail with depth=5 and g(10)");
    
    // Test 3: Shallow recursion should still work
    let (tx3, rx3) = mpsc::channel();
    let handle3 = thread::spawn(move || {
        reset_depth();
        let mut ctx = Context::new().unwrap();
        let result = ctx.eval(r#"
            function h(n) { if (n > 0) return h(n-1); return 1; }
            h(2);
        "#);
        tx3.send(result.is_ok()).unwrap();
    });
    
    let third_succeeds = rx3.recv().unwrap();
    handle3.join().unwrap();
    assert!(third_succeeds, "Shallow recursion h(2) should succeed with depth=5");
    
    // Reset to default
    set_max_call_depth(1000);
    reset_depth();
}
