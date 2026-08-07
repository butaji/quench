//! Reproducer: native static methods must work in tail-return position.

#[cfg(test)]
mod tests {
    use crate::Context;
    use crate::Value;

    fn eval_builtins(src: &str) -> Result<Value, crate::value::JsError> {
        let mut ctx = Context::new().unwrap();
        crate::builtins::register_builtins(&mut ctx);
        ctx.eval(src)
    }

    #[test]
    fn object_define_property_tail_return_from_user_function() {
        let r = eval_builtins(
            "var o = {}; var f = function(t, k, d) { return Object.defineProperty(t, k, d); }; \
             f(o, 'x', {value: 42, writable: true, enumerable: true, configurable: true}); o.x",
        )
        .unwrap();
        assert_eq!(r, Value::Number(42.0));
    }

    #[test]
    fn reflect_define_property_tail_return_from_user_function() {
        let r = eval_builtins(
            "var o = {}; var f = function(t, k, d) { return Reflect.defineProperty(t, k, d); }; \
             f(o, 'x', {value: 42, writable: true, enumerable: true, configurable: true}); o.x",
        )
        .unwrap();
        assert_eq!(r, Value::Number(42.0));
    }
}
