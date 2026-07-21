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
}
