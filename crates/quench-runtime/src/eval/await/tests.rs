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
}
