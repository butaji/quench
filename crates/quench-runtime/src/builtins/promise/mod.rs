//! Promise built-in module

mod callbacks;
mod constructor;
mod helpers;
mod instance_methods;
mod microtask;
mod static_methods;

// Re-export public APIs
pub(crate) use callbacks::enqueue_promise_reactions;
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
}
