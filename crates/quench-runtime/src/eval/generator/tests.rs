//! Unit tests for generator function state management.
//!
//! Generator function invocation returns a GeneratorObject.
//! Full .next/.return/.throw support is registered separately.

#[cfg(test)]
mod generator_tests {
    use crate::{Context, Value};

    fn eval(src: &str) -> Result<Value, crate::value::JsError> {
        Context::new().unwrap().eval(src)
    }

    // ─── Generator function detection ─────────────────────────────────────────

    #[test]
    fn generator_function_kind_is_generator() {
        // function* has kind "generator"
        let r = eval("var g = (function*() {}); typeof g;").unwrap();
        assert_eq!(r, Value::String("function".to_string()));
    }

    // ─── Generator state ─────────────────────────────────────────────────────

    #[test]
    fn generator_returns_object() {
        let r = eval("function* g() {} typeof g()").unwrap();
        assert_eq!(r, Value::String("object".to_string()));
    }

    #[test]
    fn generator_next_without_value() {
        let r = eval("function* g() { yield 1; } g().next().value").unwrap();
        assert_eq!(r, Value::Number(1.0));
    }

    #[test]
    fn generator_next_yields_correct_values() {
        let r = eval("function* g() { yield 1; yield 2; } var gen = g(); gen.next().value + gen.next().value").unwrap();
        assert_eq!(r, Value::Number(3.0));
    }

    #[test]
    fn generator_return_done() {
        let r = eval("function* g() { return 42; } g().next().done").unwrap();
        assert_eq!(r, Value::Boolean(true));
    }

    #[test]
    fn generator_multiple_yields() {
        let r = eval(
            "function* g() { yield 1; yield 2; yield 3; } \
             var gen = g(); \
             [gen.next().value, gen.next().value, gen.next().value];",
        )
        .unwrap();
        assert!(matches!(r, Value::Object(_)));
    }

    #[test]
    fn generator_body_not_executed_on_creation() {
        let r = eval(
            "var executed = false; \
             function* g() { executed = true; } \
             var gen = g(); \
             executed;",
        )
        .unwrap();
        assert_eq!(r, Value::Boolean(false));
    }

    // ─── Async generator ──────────────────────────────────────────────────────

    #[test]
    fn async_generator_returns_object() {
        let r = eval("async function* g() {} typeof g()").unwrap();
        assert_eq!(r, Value::String("object".to_string()));
    }

    #[test]
    fn async_generator_has_next_method() {
        let r = eval("async function* g() {} var gen = g(); typeof gen.next").unwrap();
        assert_eq!(r, Value::String("function".to_string()));
    }

    #[test]
    fn async_generator_has_return_method() {
        let r = eval("async function* g() {} var gen = g(); typeof gen.return").unwrap();
        assert_eq!(r, Value::String("function".to_string()));
    }

    #[test]
    fn async_generator_has_throw_method() {
        let r = eval("async function* g() {} var gen = g(); typeof gen.throw").unwrap();
        assert_eq!(r, Value::String("function".to_string()));
    }

    #[test]
    fn async_generator_next_returns_promise() {
        let r = eval(
            "async function* g() { yield 1; } \
             var gen = g(); \
             gen.next() instanceof Promise;",
        )
        .unwrap();
        assert_eq!(r, Value::Boolean(true));
    }
}
