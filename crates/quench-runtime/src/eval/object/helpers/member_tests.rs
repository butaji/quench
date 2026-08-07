//! Unit tests for object member access helpers.

#[cfg(test)]
mod helper_tests {
    use crate::{Context, Value};

    fn eval(src: &str) -> Result<Value, crate::value::JsError> {
        Context::new().unwrap().eval(src)
    }

    // ─── name_matches_prop ─────────────────────────────────────────────────────

    #[test]
    fn name_matches_ident() {
        let _r = eval("var o = {}; o.prop = 1;").unwrap();
        // name_matches_prop is used internally by class static method lookup
        let r2 = eval(
            "class C { static foo() { return 1; } } \
             C.foo();",
        )
        .unwrap();
        assert_eq!(r2, Value::Number(1.0));
    }

    #[test]
    fn name_matches_string_key() {
        let r = eval(
            "class C {}; \
             C['bar'] = function() { return 2; }; \
             C.bar();",
        )
        .unwrap();
        assert_eq!(r, Value::Number(2.0));
    }

    #[test]
    fn name_matches_number_key() {
        let r = eval(
            "class C {}; \
             C[123] = function() { return 3; }; \
             C[123]();",
        )
        .unwrap();
        assert_eq!(r, Value::Number(3.0));
    }

    #[test]
    fn name_matches_computed_is_false() {
        // Computed property keys in class static methods are not matched by name
        let r = eval("class C {}; C.missing === undefined;").unwrap();
        assert_eq!(r, Value::Boolean(true));
    }

    // ─── get_number_method ────────────────────────────────────────────────────

    #[test]
    fn get_number_method_to_fixed() {
        let r = eval("(42).toFixed(1);").unwrap();
        assert_eq!(r, Value::String("42.0".to_string()));
    }

    #[test]
    fn get_number_method_constructor() {
        let r = eval("(42).constructor === Number;").unwrap();
        assert_eq!(r, Value::Boolean(true));
    }

    // ─── has_readonly_prototype_property ──────────────────────────────────────

    #[test]
    fn readonly_builtin_property() {
        // Object.prototype.toString is not writable
        let r = eval(
            "var desc = Object.getOwnPropertyDescriptor(Object.prototype, 'toString'); \
             desc !== undefined;",
        )
        .unwrap();
        assert_eq!(r, Value::Boolean(true));
    }

    // ─── is_readonly_constructor_property ─────────────────────────────────────

    #[test]
    fn readonly_number_static_property() {
        let r = eval("Number.MAX_VALUE !== undefined;").unwrap();
        assert_eq!(r, Value::Boolean(true));
    }

    #[test]
    fn readonly_number_nan() {
        let r = eval("Number.NaN !== undefined;").unwrap();
        assert_eq!(r, Value::Boolean(true));
    }

    #[test]
    fn readonly_length_property() {
        // Function.length is readonly
        let r = eval(
            "var desc = Object.getOwnPropertyDescriptor(function(){}, 'length'); \
             desc !== undefined && desc.writable === false;",
        )
        .unwrap();
        assert_eq!(r, Value::Boolean(true));
    }

    // ─── get_member_function ──────────────────────────────────────────────────

    #[test]
    fn get_member_function_object_method() {
        let r = eval(
            "var o = {fn: function() { return 42; }}; \
             var fn = o.fn; \
             fn();",
        )
        .unwrap();
        assert_eq!(r, Value::Number(42.0));
    }

    #[test]
    fn get_member_function_undefined_for_non_object() {
        let r = eval("var u = null; typeof u;").unwrap();
        assert_eq!(r, Value::String("object".to_string()));
    }
}
