//! Tests for Promise, Math.random, and microtask queue

use quench_runtime::{Context, Value};

#[cfg(test)]
mod tests {
    use super::*;

    // =======================================================================
    // Promise tests
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
    fn test_promise_then_returns_promise() {
        let mut ctx = Context::new().unwrap();
        let result = ctx.eval("Promise.resolve(42).then(function(x) { return x * 2; })");
        assert!(result.is_ok(), "Promise.then should return a promise: {:?}", result);
    }

    #[test]
    fn test_promise_catch() {
        let mut ctx = Context::new().unwrap();
        let result = ctx.eval("Promise.reject('error').catch(function(e) { return 'caught'; })");
        assert!(result.is_ok(), "Promise.catch should work: {:?}", result);
    }

    #[test]
    fn test_promise_finally() {
        let mut ctx = Context::new().unwrap();
        let result = ctx.eval("Promise.resolve(42).finally(function() { return 1; })");
        assert!(result.is_ok(), "Promise.finally should work: {:?}", result);
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
    fn test_math_random_produces_different_values() {
        let mut ctx = Context::new().unwrap();
        let result = ctx.eval("Math.random() !== Math.random()");
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

    // =======================================================================
    // Promise static methods tests
    // =======================================================================

    #[test]
    fn test_promise_all() {
        let mut ctx = Context::new().unwrap();
        let result = ctx.eval("typeof Promise.all");
        assert_eq!(result.unwrap(), Value::String("function".to_string()));
    }

    #[test]
    fn test_promise_race() {
        let mut ctx = Context::new().unwrap();
        let result = ctx.eval("typeof Promise.race");
        assert_eq!(result.unwrap(), Value::String("function".to_string()));
    }

    #[test]
    fn test_promise_all_settled() {
        let mut ctx = Context::new().unwrap();
        let result = ctx.eval("typeof Promise.allSettled");
        assert_eq!(result.unwrap(), Value::String("function".to_string()));
    }

    #[test]
    fn test_promise_any() {
        let mut ctx = Context::new().unwrap();
        let result = ctx.eval("typeof Promise.any");
        assert_eq!(result.unwrap(), Value::String("function".to_string()));
    }
}
