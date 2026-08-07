//! Promise built-in module

mod callbacks;
mod constructor;
mod helpers;
mod instance_methods;
mod microtask;
mod static_methods;

// Re-export submodules of static_methods
pub use static_methods::{capability, promise_all, promise_race};

// Re-export public APIs
pub use constructor::{create_promise_constructor, register_promise};
pub use helpers::{
    clear_promise_proto, create_callback_promise, create_promise_proto, create_rejected_promise,
    create_resolved_promise, get_promise_proto, set_promise_proto,
};
pub use microtask::{execute_pending_microtasks, queue_microtask_impl};
pub use static_methods::{
    promise_all_impl, promise_race_impl, promise_reject_impl_static, promise_resolve_impl_static,
};

// Re-export instance method for internal use
pub use instance_methods::promise_then_impl;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::value::object::PromiseObjectData;
    use crate::value::object::PromiseState;

    #[test]
    fn test_promise_state_pending() {
        let data = PromiseObjectData::new();
        assert_eq!(data.state, PromiseState::Pending);
    }

    #[test]
    fn test_promise_fulfill() {
        let mut data = PromiseObjectData::new();
        data.fulfill(crate::value::Value::Number(42.0));
        assert_eq!(data.state, PromiseState::Fulfilled);
        assert_eq!(data.result, crate::value::Value::Number(42.0));
    }

    #[test]
    fn test_promise_reject() {
        let mut data = PromiseObjectData::new();
        data.reject(crate::value::Value::String("error".to_string()));
        assert_eq!(data.state, PromiseState::Rejected);
        assert_eq!(
            data.result,
            crate::value::Value::String("error".to_string())
        );
    }

    #[test]
    fn test_create_resolved_promise() {
        let promise = create_resolved_promise(crate::value::Value::Number(42.0));
        let obj = promise.borrow();
        if let Some(ref data) = obj.promise_data {
            assert_eq!(data.state, PromiseState::Fulfilled);
            assert_eq!(data.result, crate::value::Value::Number(42.0));
        }
    }

    #[test]
    fn test_create_rejected_promise() {
        let promise =
            create_rejected_promise(crate::value::Value::String("error".to_string())).unwrap();
        let obj = promise.borrow();
        if let Some(ref data) = obj.promise_data {
            assert_eq!(data.state, PromiseState::Rejected);
            assert_eq!(
                data.result,
                crate::value::Value::String("error".to_string())
            );
        }
    }

    #[test]
    fn test_promise_reactions_run_as_microtasks() {
        let mut ctx = crate::Context::new().unwrap();
        ctx.eval("var o = []; Promise.resolve().then(() => o.push(1)); o.push(2);")
            .unwrap();
        // The reaction must run AFTER the synchronous script (o == [2, 1])
        let result = ctx.eval("o.join(',')").unwrap();
        assert_eq!(result, crate::value::Value::String("2,1".to_string()));
    }

    #[test]
    fn test_queue_microtask_runs_at_checkpoint() {
        let mut ctx = crate::Context::new().unwrap();
        ctx.eval("var q = []; queueMicrotask(() => q.push(1)); q.push(2);")
            .unwrap();
        let result = ctx.eval("q.join(',')").unwrap();
        assert_eq!(result, crate::value::Value::String("2,1".to_string()));
    }

    #[test]
    fn test_promise_race_resolved_input() {
        let mut ctx = crate::Context::new().unwrap();
        ctx.eval("var r = 0; Promise.race([Promise.resolve(1)]).then(v => r = v);")
            .unwrap();
        // Drain microtasks between eval calls so the .then callback runs
        crate::builtins::execute_pending_microtasks().ok();
        let result = ctx.eval("r").unwrap();
        assert_eq!(result, crate::value::Value::Number(1.0));
    }

    #[test]
    fn test_promise_all_non_promise_elements() {
        let mut ctx = crate::Context::new().unwrap();
        ctx.eval("var a = null; Promise.all([1, {}, Promise.resolve(3)]).then(v => a = v);")
            .unwrap();
        let result = ctx
            .eval("a === null ? 'pending' : a.length + ':' + a[0] + ':' + a[2]")
            .unwrap();
        assert_eq!(result, crate::value::Value::String("3:1:3".to_string()));
    }

    #[test]
    fn test_promise_constructor_requires_executor() {
        let mut ctx = crate::Context::new().unwrap();
        let result = ctx
            .eval("try { new Promise(); 'nothrow' } catch (e) { 'throw' }")
            .unwrap();
        assert_eq!(result, crate::value::Value::String("throw".to_string()));
    }

    #[test]
    fn test_promise_resolution_unwraps_promise() {
        let mut ctx = crate::Context::new().unwrap();
        ctx.eval("var u = 0; Promise.resolve().then(() => Promise.resolve(42)).then(v => u = v);")
            .unwrap();
        // Also check if u is set right after the first eval
        let result_before = ctx.eval("u").unwrap();
        // Reset u and try again
        ctx.eval("u = 0;").unwrap();
        // Trigger microtask checkpoint again
        crate::builtins::execute_pending_microtasks().ok();
        let _result_after = ctx.eval("u").unwrap();
        assert_eq!(result_before, crate::value::Value::Number(42.0));
    }

    #[test]
    fn test_promise_finally_passthrough() {
        let mut ctx = crate::Context::new().unwrap();
        ctx.eval("var f = 0; var ran = false; Promise.resolve(7).finally(() => { ran = true; }).then(v => f = v);").unwrap();
        let result = ctx.eval("ran && f === 7").unwrap();
        assert_eq!(result, crate::value::Value::Boolean(true));
    }

    #[test]
    fn test_promise_then_returns_promise() {
        let mut ctx = crate::Context::new().unwrap();
        // Promise.resolve().then returns a Promise
        let result = ctx.eval("var p = Promise.resolve(42); var q = p.then(function(v) { return v * 2; }); q instanceof Promise").unwrap();
        assert_eq!(
            result,
            crate::value::Value::Boolean(true),
            "promise.then() should return a Promise"
        );
    }

    #[test]
    fn test_promise_then_value_chaining() {
        let mut ctx = crate::Context::new().unwrap();
        // Test that .then() chains values correctly
        ctx.eval("var results = []; Promise.resolve(1).then(function(v) { results.push(v); return v + 1; }).then(function(v) { results.push(v); return v + 1; });").unwrap();
        let result = ctx.eval("results.join(',')").unwrap();
        assert_eq!(
            result,
            crate::value::Value::String("1,2".to_string()),
            "then callbacks should chain with correct values"
        );
    }

    // Granular tests to isolate async .then() chain failure
    #[test]
    fn test_async_function_iife_returns_value() {
        let mut ctx = crate::Context::new().unwrap();
        // What does (async function() {})() return?
        let result = ctx.eval("(async function() { return 42; })()");
        assert!(result.is_ok(), "eval failed: {:?}", result);
        // If async is NOT supported, this returns the function itself (object)
        // If async IS supported, this returns a Promise, which .then() can chain
    }

    #[test]
    fn test_async_function_returns_promise_not_plain_value() {
        let mut ctx = crate::Context::new().unwrap();
        // async function() should return a Promise, not 42
        let result = ctx.eval("(async function() { return 42; })() instanceof Promise");
        assert!(result.is_ok(), "eval failed: {:?}", result);
        let is_promise = result.unwrap();
        assert_eq!(
            is_promise,
            crate::value::Value::Boolean(true),
            "async fn should return a Promise, got {:?}",
            is_promise
        );
    }

    #[test]
    fn test_async_function_return_value_wrapped_in_promise() {
        let mut ctx = crate::Context::new().unwrap();
        // The return value of an async function should be wrapped in a resolved Promise
        let result = ctx.eval(
            r#"
            (async function() { return 42; })()
                .then(function(v) { return v; });
        "#,
        );
        assert!(result.is_ok(), "eval failed: {:?}", result);
    }

    #[test]
    fn test_async_function_result_has_then() {
        let mut ctx = crate::Context::new().unwrap();
        let result = ctx.eval("(async function() {})().then");
        assert!(result.is_ok(), "then property should exist: {:?}", result);
        let then_prop = result.unwrap();
        // If .then exists it should be a function
        assert_ne!(
            then_prop,
            crate::value::Value::Undefined,
            ".then should not be undefined"
        );
    }

    #[test]
    fn test_async_function_simple_then() {
        let mut ctx = crate::Context::new().unwrap();
        // Simplest possible test: async fn .then(fn)
        let result = ctx.eval("(async function() {})().then(function() {})");
        assert!(result.is_ok(), "first .then() should work: {:?}", result);
    }

    #[test]
    fn test_async_function_then_chain() {
        let mut ctx = crate::Context::new().unwrap();
        let result = ctx.eval(
            r#"
            var results = [];
            (async function() {
                results.push('async-start');
            })()
            .then(function() {
                results.push('then1');
            })
            .then(function() {
                results.push('then2');
            });
        "#,
        );
        assert!(result.is_ok(), "eval failed: {:?}", result);
    }

    #[test]
    fn test_promise_then_on_pending_promise() {
        let mut ctx = crate::Context::new().unwrap();
        let result = ctx.eval(
            r#"
            var p = new Promise(function(resolve) { });
            var q = p.then(function() { return 1; });
            typeof q;
        "#,
        );
        assert!(result.is_ok(), "eval failed: {:?}", result);
        assert_eq!(
            result.unwrap(),
            crate::value::Value::String("object".to_string())
        );
    }
}
