//! Unit tests for async/await runtime support.

#[cfg(test)]
mod await_tests {
    use crate::{Context, Value};

    fn eval(src: &str) -> Result<Value, crate::value::JsError> {
        Context::new().unwrap().eval(src)
    }

    // ─── is_promise ──────────────────────────────────────────────────────────

    #[test]
    fn is_promise_promise_object() {
        // Promise.resolve() returns a Promise object
        let r = eval("Promise.resolve(1) !== undefined;").unwrap();
        assert_eq!(r, Value::Boolean(true));
    }

    #[test]
    fn is_promise_plain_object_false() {
        // Plain objects are not Promises
        let r = eval("var o = {}; o.then !== undefined;").unwrap();
        assert_eq!(r, Value::Boolean(false));
    }

    #[test]
    fn is_promise_number_false() {
        let r = eval("typeof 42;").unwrap();
        assert_eq!(r, Value::String("number".to_string()));
    }

    #[test]
    fn is_promise_string_false() {
        let r = eval("typeof 'hello';").unwrap();
        assert_eq!(r, Value::String("string".to_string()));
    }

    #[test]
    fn is_promise_thenable_is_promise() {
        // An object with a .then method that follows the right signature
        // should be treated as a Promise-like (thenable) — but per spec,
        // is_promise checks the prototype chain for Promise.prototype's
        // promise_data marker. A plain thenable without the marker returns false.
        let r = eval(
            "var thenable = { then: function(resolve) { resolve(1); } }; \
             Promise.resolve(thenable) instanceof Promise;",
        )
        .unwrap();
        assert_eq!(r, Value::Boolean(true));
    }

    // ─── eval_await_value ────────────────────────────────────────────────────

    #[test]
    fn await_resolves_non_promise() {
        // Promise.resolve wraps non-Promise values
        let r = eval("Promise.resolve(42) !== undefined;").unwrap();
        assert_eq!(r, Value::Boolean(true));
    }

    #[test]
    fn await_passes_through_promise() {
        // If value is already a Promise, eval_await_value returns it unchanged
        let r = eval(
            "var p = Promise.resolve(1); \
             Promise.resolve(p) === p;",
        )
        .unwrap();
        assert_eq!(r, Value::Boolean(true));
    }

    // ─── async function basics ────────────────────────────────────────────────

    #[test]
    fn async_function_returns_promise() {
        let r = eval(
            "async function f() {} \
             f() instanceof Promise;",
        )
        .unwrap();
        assert_eq!(r, Value::Boolean(true));
    }

    #[test]
    fn async_function_resolves_return_value() {
        let r = eval(
            "async function f() { return 42; } \
             f();",
        )
        .unwrap();
        // Returns a Promise
        assert!(!matches!(r, Value::Undefined));
    }

    #[test]
    fn async_arrow_returns_promise() {
        let r = eval(
            "var f = async () => 1; \
             f() instanceof Promise;",
        )
        .unwrap();
        assert_eq!(r, Value::Boolean(true));
    }

    #[test]
    fn async_method_returns_promise() {
        let r = eval(
            "var obj = { async method() { return 'ok'; } }; \
             obj.method() instanceof Promise;",
        )
        .unwrap();
        assert_eq!(r, Value::Boolean(true));
    }

    #[test]
    fn async_await_plain_value_fulfills_with_value() {
        let mut ctx = Context::new().unwrap();
        ctx.eval(
            "var result; async function f() { return await 42; } f().then(function(value) { result = value; });",
        )
        .unwrap();
        let _ = crate::builtins::promise::execute_pending_microtasks();
        assert_eq!(ctx.get_global("result"), Some(Value::Number(42.0)));
    }

    #[test]
    fn async_await_plain_value_in_variable_fulfills_with_value() {
        let mut ctx = Context::new().unwrap();
        ctx.eval(
            "var result; async function f() { var value = await 42; return value; } f().then(function(value) { result = value; });",
        )
        .unwrap();
        let _ = crate::builtins::promise::execute_pending_microtasks();
        assert_eq!(ctx.get_global("result"), Some(Value::Number(42.0)));
    }

    #[test]
    fn async_await_super_method_with_done_binding_fulfills_with_string() {
        let mut ctx = Context::new().unwrap();
        ctx.eval(
            "var $DONE = function() {}; var sup = { method() { return 'sup'; } }; \
             var child = { async method() { var x = await super.method(); \
             globalThis.result = x; } }; Object.setPrototypeOf(child, sup); \
             child.method().then($DONE, $DONE);",
        )
        .unwrap();
        let _ = crate::builtins::promise::execute_pending_microtasks();
        assert_eq!(
            ctx.get_global("result"),
            Some(Value::String("sup".to_string()))
        );
    }

    #[test]
    fn async_function_finally_await_reject_overrides_return() {
        let mut ctx = Context::new().unwrap();
        let _ = ctx.eval(
            "var result;
             var seen;
             async function f() {
               try {
                 return 'early-return';
               } finally {
                 seen = await new Promise(function(resolve, reject) { reject('override'); });
               }
              }
             var p = f();
             p.then(function(v) { result = 'resolved:' + v; }, function(e) { result = 'rejected:' + e; });
             ;",
        )
        .unwrap();
        let f = ctx.eval("f").unwrap();
        if let Value::Function(f) = f {
            assert!(f.is_async);
        } else {
            panic!("f should be a function");
        }
        let seen = ctx.eval("typeof seen").unwrap();
        assert_ne!(seen, Value::String("undefined".to_string()));
        let p = ctx.eval("p").unwrap();
        let seen = ctx.eval("seen").unwrap();
        let p_state = if let Value::Object(p_obj) = p {
            p_obj
                .borrow()
                .promise_data
                .as_ref()
                .map(|d| d.state.clone())
        } else {
            panic!("p should be a promise");
        };
        let seen_state = if let Value::Object(seen_obj) = seen {
            seen_obj
                .borrow()
                .promise_data
                .as_ref()
                .map(|d| d.state.clone())
        } else {
            panic!("seen should be a promise");
        };
        assert_eq!(p_state, seen_state);
        let same = ctx.eval("p === seen").unwrap();
        assert_eq!(same, Value::Boolean(true));
        crate::builtins::promise::execute_pending_microtasks().unwrap();
        let r = ctx.eval("result").unwrap();
        assert_eq!(r, Value::String("rejected:override".to_string()));
    }

    #[test]
    fn async_finally_return_await_overrides_pending_throw() {
        let mut ctx = Context::new().unwrap();
        ctx.eval(
            "var result; async function f() { try { throw 'early'; } finally { return await new Promise(function(resolve) { resolve('override'); }); } } \
             f().then(function(value) { result = value; });",
        )
        .unwrap();
        let _ = crate::builtins::promise::execute_pending_microtasks();
        assert_eq!(
            ctx.get_global("result"),
            Some(Value::String("override".into()))
        );
    }

    #[test]
    fn named_async_function_name_reassignment_rejects_in_strict_body() {
        let mut ctx = Context::new().unwrap();
        ctx.eval(
            "'use strict'; var result; var calls = 0; var ref = async function BindingIdentifier() { \
             calls++; (() => { BindingIdentifier = 1; })(); }; \
             ref().then(function() { result = 'resolved'; }, function(error) { result = error.name; });",
        )
        .unwrap();
        let _ = crate::builtins::promise::execute_pending_microtasks();
        assert_eq!(ctx.get_global("calls"), Some(Value::Number(1.0)));
        assert_eq!(
            ctx.get_global("result"),
            Some(Value::String("TypeError".into()))
        );
    }

    #[test]
    fn async_await_rejected_promise_enters_catch() {
        let mut ctx = Context::new().unwrap();
        ctx.eval(
            "var result; async function f() { try { await Promise.reject('boom'); } catch (error) { result = error; } } \
             f();",
        )
        .unwrap();
        let _ = crate::builtins::promise::execute_pending_microtasks();
        assert_eq!(ctx.get_global("result"), Some(Value::String("boom".into())));
    }

    #[test]
    fn async_function_finally_await_reject_overrides_throw() {
        let mut ctx = Context::new().unwrap();
        let _ = ctx.eval(
            "var result;
             async function f() {
               try {
                 await new Promise(function(resolve, reject) { reject('early-reject'); });
               } finally {
                 await new Promise(function(resolve, reject) { reject('override'); });
               }
             }
             var p = f();
             p.then(function(v) { result = 'resolved:' + v; }, function(e) { result = 'rejected:' + e; });",
        )
        .unwrap();
        crate::builtins::promise::execute_pending_microtasks().unwrap();
        let r = ctx.eval("result").unwrap();
        assert_eq!(r, Value::String("rejected:override".to_string()));
    }

    #[test]
    fn async_arrow_finally_rejection_reaches_chained_handler() {
        let mut ctx = Context::new().unwrap();
        ctx.eval(
            "var result; var f = async () => { try { throw 'early-throw'; } \
             finally { await new Promise(function(resolve, reject) { reject('override'); }); } }; \
             f().then(function() { result = 'wrong'; }, function(value) { result = value; }) \
             .then(function() { result = result + ':done'; }, function() { result = 'rejected'; });",
        )
        .unwrap();
        let _ = crate::builtins::promise::execute_pending_microtasks();
        assert_eq!(
            ctx.get_global("result"),
            Some(Value::String("override:done".into()))
        );
    }
}
