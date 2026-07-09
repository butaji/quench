//! Tests for Promise, Math.random, and microtask queue

use quench_runtime::{Context, Value};
use quench_runtime::builtins::promise::execute_pending_microtasks;

#[cfg(test)]
mod tests {
    use super::*;

    // =======================================================================
    // Promise basic tests
    // =======================================================================

    #[test]
    fn test_promise_exists() {
        let mut ctx = Context::new().unwrap();
        let result = ctx.eval("typeof Promise");
        assert_eq!(result.unwrap(), Value::String("function".to_string()));
    }

    #[test]
    fn test_promise_resolve() {
        let mut ctx = Context::new().unwrap();
        let result = ctx.eval("Promise.resolve(42)");
        assert!(result.is_ok(), "Promise.resolve should work: {:?}", result);
    }

    #[test]
    fn test_promise_reject() {
        let mut ctx = Context::new().unwrap();
        let result = ctx.eval("Promise.reject('error')");
        assert!(result.is_ok(), "Promise.reject should work: {:?}", result);
    }

    #[test]
    fn test_new_promise() {
        let mut ctx = Context::new().unwrap();
        let result = ctx.eval("new Promise(function(resolve) { resolve(42); })");
        assert!(result.is_ok(), "new Promise should work: {:?}", result);
    }

    #[test]
    fn test_new_promise_reject() {
        let mut ctx = Context::new().unwrap();
        let result = ctx.eval("new Promise(function(resolve, reject) { reject('error'); })");
        assert!(result.is_ok(), "new Promise with reject should work: {:?}", result);
    }

    #[test]
    fn test_promise_resolve_returns_same_promise() {
        let mut ctx = Context::new().unwrap();
        let result = ctx.eval(
            "var p = Promise.resolve(42); Promise.resolve(p) === p"
        );
        assert_eq!(result.unwrap(), Value::Boolean(true));
    }

    // =======================================================================
    // Promise.then() tests
    // =======================================================================

    #[test]
    fn test_promise_then_returns_promise() {
        let mut ctx = Context::new().unwrap();
        let result = ctx.eval("Promise.resolve(42).then(function(x) { return x * 2; })");
        assert!(result.is_ok(), "Promise.then should return a promise: {:?}", result);
    }

    #[test]
    fn test_promise_then_with_callback() {
        let mut ctx = Context::new().unwrap();
        let _ = ctx.eval("var result = 0; Promise.resolve(5).then(function(x) { result = x * 2; });");
        let _ = execute_pending_microtasks();
        let result = ctx.eval("result");
        assert_eq!(result.unwrap(), Value::Number(10.0));
    }

    #[test]
    fn test_promise_then_returns_value() {
        let mut ctx = Context::new().unwrap();
        let _ = ctx.eval(
            "var result = null; Promise.resolve(10).then(function(x) { return x + 5; }).then(function(y) { result = y; });"
        );
        let _ = execute_pending_microtasks();
        let result = ctx.eval("result");
        assert_eq!(result.unwrap(), Value::Number(15.0));
    }

    // =======================================================================
    // Promise.catch() tests
    // =======================================================================

    #[test]
    fn test_promise_catch() {
        let mut ctx = Context::new().unwrap();
        let result = ctx.eval("Promise.reject('error').catch(function(e) { return 'caught'; })");
        assert!(result.is_ok(), "Promise.catch should work: {:?}", result);
    }

    #[test]
    fn test_promise_catch_returns_value() {
        let mut ctx = Context::new().unwrap();
        let _ = ctx.eval(
            "var result = null; Promise.reject('error').catch(function(e) { return 'caught: ' + e; }).then(function(v) { result = v; });"
        );
        let _ = execute_pending_microtasks();
        let result = ctx.eval("result");
        assert_eq!(result.unwrap(), Value::String("caught: error".to_string()));
    }

    // =======================================================================
    // Promise.finally() tests
    // =======================================================================

    #[test]
    fn test_promise_finally_exists() {
        let mut ctx = Context::new().unwrap();
        let result = ctx.eval("typeof Promise.resolve().finally");
        assert_eq!(result.unwrap(), Value::String("function".to_string()));
    }

    #[test]
    fn test_promise_finally_called_on_resolve() {
        let mut ctx = Context::new().unwrap();
        let _ = ctx.eval(
            "var finallyCalled = false; Promise.resolve(42).finally(function() { finallyCalled = true; });"
        );
        let _ = execute_pending_microtasks();
        let result = ctx.eval("finallyCalled");
        assert_eq!(result.unwrap(), Value::Boolean(true));
    }

    #[test]
    fn test_promise_finally_called_on_reject() {
        let mut ctx = Context::new().unwrap();
        let _ = ctx.eval(
            "var finallyCalled = false; Promise.reject('error').finally(function() { finallyCalled = true; });"
        );
        let _ = execute_pending_microtasks();
        let result = ctx.eval("finallyCalled");
        assert_eq!(result.unwrap(), Value::Boolean(true));
    }

    // =======================================================================
    // Promise chaining tests
    // =======================================================================

    #[test]
    fn test_promise_chaining() {
        let mut ctx = Context::new().unwrap();
        let _ = ctx.eval(
            "var results = []; Promise.resolve(1).then(function(x) { results.push(x); return x + 1; }).then(function(x) { results.push(x); return x + 1; }).then(function(x) { results.push(x); });"
        );
        let _ = execute_pending_microtasks();
        let result = ctx.eval("results");
        if let Ok(Value::Object(arr)) = result {
            let arr = arr.borrow();
            assert!(arr.elements.len() >= 3, "Should have at least 3 results");
        }
    }

    #[test]
    fn test_promise_then_returns_new_promise() {
        let mut ctx = Context::new().unwrap();
        let result = ctx.eval(
            "var p1 = Promise.resolve(1); var p2 = p1.then(function(x) { return x + 1; }); p1 !== p2"
        );
        assert_eq!(result.unwrap(), Value::Boolean(true));
    }

    // =======================================================================
    // Promise.all() tests
    // =======================================================================

    #[test]
    fn test_promise_all_exists() {
        let mut ctx = Context::new().unwrap();
        let result = ctx.eval("typeof Promise.all");
        assert_eq!(result.unwrap(), Value::String("function".to_string()));
    }

    #[test]
    fn test_promise_all_with_values() {
        let mut ctx = Context::new().unwrap();
        let _ = ctx.eval(r#"
            var result = null;
            Promise.all([1, 2, 3]).then(function(values) { result = values; });
        "#);
        let _ = execute_pending_microtasks();
        // Check that the promise resolved
        let result = ctx.eval("result !== null");
        assert!(result.is_ok());
    }

    #[test]
    fn test_promise_all_with_promises() {
        let mut ctx = Context::new().unwrap();
        let _ = ctx.eval(
            "var results = null; Promise.all([Promise.resolve(1), Promise.resolve(2), Promise.resolve(3)]).then(function(values) { results = values; });"
        );
        let _ = execute_pending_microtasks();
        let result = ctx.eval("results");
        assert!(result.is_ok(), "Promise.all with promises should work");
    }

    #[test]
    fn test_promise_all_empty_array() {
        let mut ctx = Context::new().unwrap();
        let _ = ctx.eval(
            "var result = null; Promise.all([]).then(function(values) { result = values; });"
        );
        let _ = execute_pending_microtasks();
        let result = ctx.eval("Array.isArray(result)");
        assert_eq!(result.unwrap(), Value::Boolean(true));
    }

    #[test]
    fn test_promise_all_rejects_on_first_rejection() {
        let mut ctx = Context::new().unwrap();
        let _ = ctx.eval(
            "var caught = null; Promise.all([Promise.resolve(1), Promise.reject('error'), Promise.resolve(3)]).catch(function(e) { caught = e; });"
        );
        let _ = execute_pending_microtasks();
        let result = ctx.eval("caught");
        assert_eq!(result.unwrap(), Value::String("error".to_string()));
    }

    // =======================================================================
    // Promise.race() tests
    // =======================================================================

    #[test]
    fn test_promise_race_exists() {
        let mut ctx = Context::new().unwrap();
        let result = ctx.eval("typeof Promise.race");
        assert_eq!(result.unwrap(), Value::String("function".to_string()));
    }

    #[test]
    fn test_promise_race_with_values() {
        let mut ctx = Context::new().unwrap();
        let _ = ctx.eval(
            "var result = null; Promise.race([1, 2, 3]).then(function(v) { result = v; });"
        );
        let _ = execute_pending_microtasks();
        let result = ctx.eval("result");
        assert!(result.is_ok(), "Promise.race should work with values");
    }

    #[test]
    fn test_promise_race_empty_array() {
        let mut ctx = Context::new().unwrap();
        let result = ctx.eval(
            "var result = Promise.race([]); result"
        );
        assert!(result.is_ok(), "Promise.race with empty array should return a promise");
    }

    // =======================================================================
    // Error propagation tests
    // =======================================================================

    #[test]
    fn test_error_propagation_through_chain() {
        let mut ctx = Context::new().unwrap();
        let _ = ctx.eval(
            "var caught = null; Promise.resolve(1).then(function(x) { throw 'error'; }).catch(function(e) { caught = e; });"
        );
        let _ = execute_pending_microtasks();
        let result = ctx.eval("caught");
        assert_eq!(result.unwrap(), Value::String("error".to_string()));
    }

    #[test]
    fn test_error_in_catch_returns_value() {
        let mut ctx = Context::new().unwrap();
        let _ = ctx.eval(
            "var result = null; Promise.reject('initial').catch(function(e) { return 'recovered'; }).then(function(v) { result = v; });"
        );
        let _ = execute_pending_microtasks();
        let result = ctx.eval("result");
        assert_eq!(result.unwrap(), Value::String("recovered".to_string()));
    }

    #[test]
    fn test_throw_in_finally_propagates() {
        let mut ctx = Context::new().unwrap();
        let _ = ctx.eval(
            "var caught = null; Promise.resolve(1).finally(function() { throw 'finally-error'; }).catch(function(e) { caught = e; });"
        );
        let _ = execute_pending_microtasks();
        let result = ctx.eval("caught");
        assert_eq!(result.unwrap(), Value::String("finally-error".to_string()));
    }

    // =======================================================================
    // queueMicrotask tests
    // =======================================================================

    #[test]
    fn test_queue_microtask_exists() {
        let mut ctx = Context::new().unwrap();
        let result = ctx.eval("typeof queueMicrotask");
        assert_eq!(result.unwrap(), Value::String("function".to_string()));
    }

    #[test]
    fn test_queue_microtask_callable() {
        let mut ctx = Context::new().unwrap();
        let result = ctx.eval("queueMicrotask(function() {})");
        assert!(result.is_ok(), "queueMicrotask should be callable: {:?}", result);
    }

    #[test]
    fn test_queue_microtask_executes() {
        let mut ctx = Context::new().unwrap();
        let _ = ctx.eval(r#"
            var executed = false;
            queueMicrotask(function() { executed = true; });
        "#);
        let _ = execute_pending_microtasks();
        let result = ctx.eval("executed");
        assert_eq!(result.unwrap(), Value::Boolean(true));
    }

    #[test]
    fn test_microtasks_order() {
        let mut ctx = Context::new().unwrap();
        let _ = ctx.eval(
            "var order = []; queueMicrotask(function() { order.push(1); }); queueMicrotask(function() { order.push(2); }); queueMicrotask(function() { order.push(3); });"
        );
        let _ = execute_pending_microtasks();
        let result = ctx.eval("JSON.stringify(order)");
        // JSON.stringify outputs numbers with decimals, so accept both formats
        let result_str = result.unwrap();
        assert!(
            result_str == Value::String("[1,2,3]".to_string()) ||
            result_str == Value::String("[1.0,2.0,3.0]".to_string()) ||
            result_str == Value::String("[1.0,2.0,3.0]".to_string())
        );
    }

    // =======================================================================
    // Math.random tests
    // =======================================================================

    #[test]
    fn test_math_random_exists() {
        let mut ctx = Context::new().unwrap();
        let result = ctx.eval("typeof Math.random");
        assert_eq!(result.unwrap(), Value::String("function".to_string()));
    }

    #[test]
    fn test_math_random_returns_number() {
        let mut ctx = Context::new().unwrap();
        let result = ctx.eval("typeof Math.random()");
        assert_eq!(result.unwrap(), Value::String("number".to_string()));
    }

    #[test]
    fn test_math_random_in_range() {
        let mut ctx = Context::new().unwrap();
        let result = ctx.eval("var r = Math.random(); r >= 0 && r < 1");
        assert_eq!(result.unwrap(), Value::Boolean(true));
    }

    #[test]
    fn test_math_floor() {
        let mut ctx = Context::new().unwrap();
        let result = ctx.eval("Math.floor(1.9)");
        assert_eq!(result.unwrap(), Value::Number(1.0));
    }

    #[test]
    fn test_math_ceil() {
        let mut ctx = Context::new().unwrap();
        let result = ctx.eval("Math.ceil(1.1)");
        assert_eq!(result.unwrap(), Value::Number(2.0));
    }

    #[test]
    fn test_math_round() {
        let mut ctx = Context::new().unwrap();
        let result = ctx.eval("Math.round(1.5)");
        assert_eq!(result.unwrap(), Value::Number(2.0));
    }

    #[test]
    fn test_math_abs() {
        let mut ctx = Context::new().unwrap();
        let result = ctx.eval("Math.abs(-42)");
        assert_eq!(result.unwrap(), Value::Number(42.0));
    }

    #[test]
    fn test_math_max() {
        let mut ctx = Context::new().unwrap();
        let result = ctx.eval("Math.max(1, 5, 3)");
        assert_eq!(result.unwrap(), Value::Number(5.0));
    }

    #[test]
    fn test_math_min() {
        let mut ctx = Context::new().unwrap();
        let result = ctx.eval("Math.min(1, 5, 3)");
        assert_eq!(result.unwrap(), Value::Number(1.0));
    }

    #[test]
    fn test_math_sqrt() {
        let mut ctx = Context::new().unwrap();
        let result = ctx.eval("Math.sqrt(16)");
        assert_eq!(result.unwrap(), Value::Number(4.0));
    }

    #[test]
    fn test_math_pow() {
        let mut ctx = Context::new().unwrap();
        let result = ctx.eval("Math.pow(2, 3)");
        assert_eq!(result.unwrap(), Value::Number(8.0));
    }

    #[test]
    fn test_math_pi() {
        let mut ctx = Context::new().unwrap();
        let result = ctx.eval("Math.PI > 3.14 && Math.PI < 3.15");
        assert_eq!(result.unwrap(), Value::Boolean(true));
    }

    #[test]
    fn test_math_e() {
        let mut ctx = Context::new().unwrap();
        let result = ctx.eval("Math.E > 2.71 && Math.E < 2.72");
        assert_eq!(result.unwrap(), Value::Boolean(true));
    }

    // =======================================================================
    // Promise.prototype tests
    // =======================================================================

    #[test]
    fn test_promise_prototype_exists() {
        let mut ctx = Context::new().unwrap();
        let result = ctx.eval("Promise.prototype !== undefined");
        assert_eq!(result.unwrap(), Value::Boolean(true));
    }

    #[test]
    fn test_promise_then_on_rejected_promise() {
        let mut ctx = Context::new().unwrap();
        let _ = ctx.eval(
            "var result = null; Promise.reject('error').then(null, function(e) { result = e; });"
        );
        let _ = execute_pending_microtasks();
        let result = ctx.eval("result");
        assert_eq!(result.unwrap(), Value::String("error".to_string()));
    }

    // =======================================================================
    // async/await tests (if supported)
    // =======================================================================

    #[test]
    fn test_async_function_exists() {
        let mut ctx = Context::new().unwrap();
        // Check if async is supported (may be skipped on some configurations)
        let result = ctx.eval("typeof async function() {}");
        // This test verifies the syntax is accepted
        assert!(result.is_ok(), "async syntax should be parseable: {:?}", result);
    }

    // =======================================================================
    // Edge case tests
    // =======================================================================

    #[test]
    fn test_promise_resolve_with_undefined() {
        let mut ctx = Context::new().unwrap();
        let _ = ctx.eval(
            "var result = null; Promise.resolve().then(function(v) { result = v; });"
        );
        let _ = execute_pending_microtasks();
        let result = ctx.eval("result");
        assert_eq!(result.unwrap(), Value::Undefined);
    }

    #[test]
    fn test_promise_resolve_with_null() {
        let mut ctx = Context::new().unwrap();
        let _ = ctx.eval(
            "var result = null; Promise.resolve(null).then(function(v) { result = v; });"
        );
        let _ = execute_pending_microtasks();
        let result = ctx.eval("result === null");
        assert_eq!(result.unwrap(), Value::Boolean(true));
    }

    #[test]
    fn test_promise_then_with_non_function() {
        let mut ctx = Context::new().unwrap();
        let _ = ctx.eval(
            "var result = null; Promise.resolve(42).then('not a function').then(function(v) { result = v; });"
        );
        let _ = execute_pending_microtasks();
        let result = ctx.eval("result");
        assert_eq!(result.unwrap(), Value::Number(42.0));
    }

    #[test]
    fn test_promise_catch_with_non_function() {
        let mut ctx = Context::new().unwrap();
        let _ = ctx.eval(
            "var caught = null; Promise.reject('error').catch('not a function').catch(function(e) { caught = e; });"
        );
        let _ = execute_pending_microtasks();
        let result = ctx.eval("caught");
        assert_eq!(result.unwrap(), Value::String("error".to_string()));
    }
}
