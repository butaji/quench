//! Unit tests for member access evaluation.
//!
//! Tests the dispatch logic in `eval_member_access` (all Value type variants),
//! `eval_class_member` (prototype, name, static members, caller/arguments restriction),
//! and `box_primitive` (Number/Boolean/Symbol/BigInt → temporary object).

#[cfg(test)]
mod member_access_tests {
    use crate::{Context, Value};

    fn eval(src: &str) -> Result<Value, crate::value::JsError> {
        Context::new().unwrap().eval(src)
    }

    // ─── eval_member_access: Value::Object dispatch ─────────────────────────────

    #[test]
    fn object_member_access_found_property() {
        let r = eval("var o = {x: 42}; o.x;").unwrap();
        assert_eq!(r, Value::Number(42.0));
    }

    #[test]
    fn object_member_access_missing_property_returns_undefined() {
        let r = eval("var o = {x: 1}; o.missing;").unwrap();
        assert_eq!(r, Value::Undefined);
    }

    #[test]
    fn object_member_access_inherited_property() {
        let r = eval("var p = {y: 10}; var o = Object.create(p); o.y;").unwrap();
        assert_eq!(r, Value::Number(10.0));
    }

    #[test]
    fn object_member_access_getter_called() {
        let r = eval("var o = {get prop() { return 99; }}; o.prop;").unwrap();
        assert_eq!(r, Value::Number(99.0));
    }

    #[test]
    fn object_member_access_getter_receives_correct_this() {
        let r = eval("var o = {v: 7, get val() { return this.v; }}; o.val;").unwrap();
        assert_eq!(r, Value::Number(7.0));
    }

    #[test]
    fn object_member_access_array_element() {
        let r = eval("var a = [1, 2, 3]; a[1];").unwrap();
        assert_eq!(r, Value::Number(2.0));
    }

    #[test]
    fn object_member_access_numeric_string_index() {
        let r = eval("var o = {0: 'zero', 1: 'one'}; o['0'];").unwrap();
        assert_eq!(r, Value::String("zero".to_string()));
    }

    #[test]
    fn object_member_access_proto_returns_prototype() {
        let r =
            eval("var p = {}; var o = Object.create(p); Object.getPrototypeOf(o) === p;").unwrap();
        assert_eq!(r, Value::Boolean(true));
    }

    // ─── eval_member_access: Value::String dispatch ──────────────────────────────

    #[test]
    fn string_member_access_length() {
        let r = eval("'hello'.length;").unwrap();
        assert_eq!(r, Value::Number(5.0));
    }

    #[test]
    fn string_member_access_char_at() {
        let r = eval("'hello'.charAt(1);").unwrap();
        assert_eq!(r, Value::String("e".to_string()));
    }

    #[test]
    fn string_member_access_char_code_at() {
        let r = eval("'abc'.charCodeAt(0);").unwrap();
        assert_eq!(r, Value::Number(97.0));
    }

    #[test]
    fn string_member_access_constructor() {
        let r = eval("'hello'.constructor === String;").unwrap();
        assert_eq!(r, Value::Boolean(true));
    }

    // ─── eval_member_access: Value::Function dispatch ────────────────────────────

    #[test]
    fn function_member_access_name() {
        let r = eval("(function foo() {}).name;").unwrap();
        assert_eq!(r, Value::String("foo".to_string()));
    }

    #[test]
    fn function_member_access_anonymous_name() {
        let r = eval("(function() {}).name;").unwrap();
        assert_eq!(r, Value::String("".to_string()));
    }

    #[test]
    fn function_member_access_length_returns_param_count() {
        let r = eval("(function(a, b, c) {}).length;").unwrap();
        assert_eq!(r, Value::Number(3.0));
    }

    #[test]
    fn function_member_access_length_stops_at_first_default() {
        let r = eval("(function(a, b = 1, c) {}).length;").unwrap();
        assert_eq!(r, Value::Number(1.0));
    }

    #[test]
    fn function_member_access_prototype() {
        let r = eval("(function() {}).prototype !== undefined;").unwrap();
        assert_eq!(r, Value::Boolean(true));
    }

    #[test]
    fn function_member_access_call_method() {
        let r = eval("(function(x) { return x + 1; }).call(null, 5);").unwrap();
        assert_eq!(r, Value::Number(6.0));
    }

    #[test]
    fn function_member_access_call_sets_this() {
        let r = eval(
            "var obj = {v: 10}; \
             (function() { return this.v; }).call(obj);",
        )
        .unwrap();
        assert_eq!(r, Value::Number(10.0));
    }

    #[test]
    fn function_member_access_apply_method() {
        let r = eval("(function(a, b) { return a + b; }).apply(null, [3, 4]);").unwrap();
        assert_eq!(r, Value::Number(7.0));
    }

    #[test]
    fn function_member_access_apply_with_args_array() {
        let r = eval("(function(a, b) { return a * b; }).apply(null, [2, 5]);").unwrap();
        assert_eq!(r, Value::Number(10.0));
    }

    #[test]
    fn function_member_access_bind_method() {
        let r = eval("(function(x, y) { return x + y; }).bind(null, 10)(5);").unwrap();
        assert_eq!(r, Value::Number(15.0));
    }

    #[test]
    fn function_member_access_bind_partial() {
        let r = eval("(function(a, b) { return a - b; }).bind(null, 10)(3);").unwrap();
        assert_eq!(r, Value::Number(7.0));
    }

    #[test]
    fn function_member_access_arrow_restricted_arguments() {
        let r = eval("var f = () => arguments; f();");
        assert!(
            r.is_err(),
            "arrow function accessing arguments should throw"
        );
    }

    #[test]
    fn function_member_access_arrow_restricted_caller() {
        let r = eval("var f = () => caller; f();");
        assert!(r.is_err(), "arrow function accessing caller should throw");
    }

    // ─── eval_member_access: Value::NativeFunction dispatch ──────────────────────

    #[test]
    fn native_function_member_access_name() {
        let r = eval("isFinite.name;").unwrap();
        assert_eq!(r, Value::String("isFinite".to_string()));
    }

    #[test]
    fn native_function_member_access_call_method() {
        let r = eval("isFinite.call(null, 42);").unwrap();
        assert_eq!(r, Value::Boolean(true));
    }

    #[test]
    fn native_function_member_access_bind_method() {
        let r = eval("parseInt.bind(null, '10')();").unwrap();
        assert_eq!(r, Value::Number(10.0));
    }

    #[test]
    fn native_function_member_access_length() {
        let r = eval("isFinite.length;").unwrap();
        assert_eq!(r, Value::Number(0.0));
    }

    // ─── eval_member_access: Value::NativeConstructor dispatch ─────────────────

    #[test]
    fn native_constructor_member_access_prototype() {
        let r = eval("Array.prototype !== undefined;").unwrap();
        assert_eq!(r, Value::Boolean(true));
    }

    #[test]
    fn native_constructor_member_access_name() {
        let r = eval("Array.name;").unwrap();
        assert_eq!(r, Value::String("Array".to_string()));
    }

    #[test]
    fn native_constructor_member_access_from() {
        let r = eval("Array.from !== undefined;").unwrap();
        assert_eq!(r, Value::Boolean(true));
    }

    // ─── eval_member_access: boxed primitives (Number/Boolean/Symbol/BigInt) ────

    #[test]
    fn boxed_number_member_access_to_fixed() {
        let r = eval("(42).toFixed(1);").unwrap();
        assert_eq!(r, Value::String("42.0".to_string()));
    }

    #[test]
    fn boxed_number_member_access_constructor() {
        let r = eval("(42).constructor === Number;").unwrap();
        assert_eq!(r, Value::Boolean(true));
    }

    // ─── eval_member_access: Value::Class dispatch ──────────────────────────────

    #[test]
    fn class_member_access_prototype() {
        let r = eval("class C {} C.prototype !== undefined;").unwrap();
        assert_eq!(r, Value::Boolean(true));
    }

    #[test]
    fn class_member_access_name() {
        let r = eval("class Foo {} Foo.name;").unwrap();
        assert_eq!(r, Value::String("Foo".to_string()));
    }

    #[test]
    fn class_member_access_anonymous_class_name() {
        let _ = eval("class {}").unwrap();
        // anonymous class expression has name ""
        let r2 = eval("(class {}).name;").unwrap();
        assert_eq!(r2, Value::String("".to_string()));
    }

    #[test]
    fn class_member_access_static_method() {
        let r = eval(
            "class C { static foo() { return 42; } } \
             C.foo();",
        )
        .unwrap();
        assert_eq!(r, Value::Number(42.0));
    }

    #[test]
    fn class_member_access_static_field() {
        let r = eval("class C { static x = 7; } C.x;").unwrap();
        assert_eq!(r, Value::Number(7.0));
    }

    #[test]
    fn class_member_access_caller_throws() {
        let r = eval("class C {} C.caller;");
        assert!(
            r.is_err(),
            "accessing caller on class constructor should throw"
        );
    }

    #[test]
    fn class_member_access_arguments_throws() {
        let r = eval("class C {} C.arguments;");
        assert!(
            r.is_err(),
            "accessing arguments on class constructor should throw"
        );
    }

    #[test]
    fn class_member_access_missing_returns_undefined() {
        let r = eval("class C {} C.missing;").unwrap();
        assert_eq!(r, Value::Undefined);
    }

    #[test]
    fn class_member_access_super_chain() {
        let r = eval(
            "class Base { static foo() { return 1; } } \
             class Child extends Base {} \
             Child.foo();",
        )
        .unwrap();
        assert_eq!(r, Value::Number(1.0));
    }

    // ─── eval_member_access: Value::Null / Value::Undefined ────────────────────

    #[test]
    fn null_member_access_throws_typeerror() {
        let r = eval("var n = null; n.foo;");
        assert!(r.is_err(), "member access on null should throw");
        let err = r.unwrap_err();
        let msg = format!("{}", err);
        assert!(msg.contains("null"), "error should mention null: {}", msg);
    }

    #[test]
    fn undefined_member_access_throws_typeerror() {
        let r = eval("var u = undefined; u.foo;");
        assert!(r.is_err(), "member access on undefined should throw");
        let err = r.unwrap_err();
        let msg = format!("{}", err);
        assert!(
            msg.contains("undefined"),
            "error should mention undefined: {}",
            msg
        );
    }

    // ─── get_prototype_from_class_val ──────────────────────────────────────────

    #[test]
    fn get_prototype_from_object() {
        let r = eval("var o = {}; Object.getPrototypeOf(o) === Object.prototype;").unwrap();
        assert_eq!(r, Value::Boolean(true));
    }

    // ─── prop_key_matches (indirectly tested via class static method lookup) ───

    #[test]
    fn class_static_method_key_matching_ident() {
        let r = eval(
            "class C { static foo() { return 1; } } \
             C.foo();",
        )
        .unwrap();
        assert_eq!(r, Value::Number(1.0));
    }

    #[test]
    fn class_static_method_key_matching_string() {
        let r = eval(
            "class C { } \
             C['bar'] = function() { return 2; }; \
             C.bar();",
        )
        .unwrap();
        assert_eq!(r, Value::Number(2.0));
    }

    // ─── Class static methods ─────────────────────────────────────────────────

    #[test]
    fn class_static_method_with_params() {
        let r = eval(
            "class C { static add(a, b) { return a + b; } } \
             C.add(3, 4);",
        )
        .unwrap();
        assert_eq!(r, Value::Number(7.0));
    }

    #[test]
    fn class_static_method_multiple_methods() {
        let r = eval(
            "class C { \
               static first() { return 'a'; } \
               static second() { return 'b'; } \
             } \
             C.first() + C.second();",
        )
        .unwrap();
        assert_eq!(r, Value::String("ab".to_string()));
    }

    #[test]
    fn class_static_method_numeric_key() {
        let r = eval(
            "class C { static 123() { return 9; } } \
             C[123]();",
        )
        .unwrap();
        assert_eq!(r, Value::Number(9.0));
    }

    #[test]
    fn class_static_method_computed_key_returns_undefined() {
        // Computed keys are not matched by prop_key_matches (returns false).
        let r = eval("class C { static [Symbol.iterator]() { return 1; } } C[Symbol.iterator];")
            .unwrap();
        assert_eq!(r, Value::Undefined);
    }

    #[test]
    fn class_static_method_length_property() {
        // C.length is the constructor's formal param count, not static method's.
        let r = eval("class C { constructor(a, b, c) {} } C.length;").unwrap();
        assert_eq!(r, Value::Number(3.0));
    }

    // ─── Class static getters ──────────────────────────────────────────────────

    #[test]
    fn class_static_getter_returns_value() {
        let r = eval(
            "class C { static get x() { return 42; } } \
             C.x;",
        )
        .unwrap();
        assert_eq!(r, Value::Number(42.0));
    }

    #[test]
    fn class_static_getter_receives_class_as_this() {
        // `this` in static getter is the class constructor object.
        let r = eval(
            "class C { static get val() { return typeof this; } } \
             C.val;",
        )
        .unwrap();
        assert_eq!(r, Value::String("object".to_string()));
    }

    #[test]
    fn class_static_getter_class_scope() {
        // Static getter body has class-body lexical scope.
        let r = eval(
            "class C { \
               static get computed() { return 99 * 2; } \
             } \
             C.computed;",
        )
        .unwrap();
        assert_eq!(r, Value::Number(198.0));
    }

    #[test]
    fn class_static_getter_with_params() {
        // Static getter is accessed like any property; getter function takes no params.
        let r = eval(
            "class C { static get prop() { return 55; } } \
             C.prop;",
        )
        .unwrap();
        assert_eq!(r, Value::Number(55.0));
    }

    #[test]
    fn class_static_getter_multiple_getters() {
        let r = eval(
            "class C { \
               static get a() { return 1; } \
               static get b() { return 2; } \
             } \
             C.a + C.b;",
        )
        .unwrap();
        assert_eq!(r, Value::Number(3.0));
    }

    // ─── Class static setters ──────────────────────────────────────────────────

    #[test]
    fn class_static_setter_returns_function() {
        // Static setter is returned as a ValueFunction.
        let r = eval(
            "class C { static set x(v) { this._x = v; } } \
             typeof C.x;",
        )
        .unwrap();
        assert_eq!(r, Value::String("function".to_string()));
    }

    #[test]
    fn class_static_setter_is_not_undefined() {
        // Static setter always exists as a function, not undefined.
        let r = eval(
            "class C { static set prop(v) {} } \
             C.prop !== undefined;",
        )
        .unwrap();
        assert_eq!(r, Value::Boolean(true));
    }

    #[test]
    fn class_static_setter_invoked_via_assignment() {
        // Assignment to a static setter property invokes the setter.
        let r = eval(
            "class C { \
               static set x(v) { return v; } \
             } \
             C.x = 77; \
             typeof C.x;",
        )
        .unwrap();
        assert_eq!(r, Value::String("function".to_string()));
    }

    #[test]
    fn class_static_setter_accessed_as_function() {
        // Accessing a static setter returns the setter function.
        let r = eval(
            "class C { \
               static set val(v) { return v * 3; } \
             } \
             var f = C.val; \
             f(10);",
        )
        .unwrap();
        assert_eq!(r, Value::Number(30.0));
    }

    #[test]
    fn class_static_setter_multiple_setters() {
        // Multiple static setters are distinct properties.
        let r = eval(
            "class C { \
               static set a(v) { return v; } \
               static set b(v) { return v; } \
             } \
             typeof C.a + typeof C.b;",
        )
        .unwrap();
        assert_eq!(r, Value::String("functionfunction".to_string()));
    }

    // ─── Class static fields ───────────────────────────────────────────────────

    #[test]
    fn class_static_field_number() {
        let r = eval("class C { static x = 7; } C.x;").unwrap();
        assert_eq!(r, Value::Number(7.0));
    }

    #[test]
    fn class_static_field_string() {
        let r = eval("class C { static msg = 'hello'; } C.msg;").unwrap();
        assert_eq!(r, Value::String("hello".to_string()));
    }

    #[test]
    fn class_static_field_boolean() {
        let r = eval("class C { static flag = true; } C.flag;").unwrap();
        assert_eq!(r, Value::Boolean(true));
    }

    #[test]
    fn class_static_field_object() {
        let r = eval("class C { static obj = {a: 1}; } C.obj.a;").unwrap();
        assert_eq!(r, Value::Number(1.0));
    }

    #[test]
    fn class_static_field_function() {
        let r = eval("class C { static fn = function() { return 42; }; } C.fn();").unwrap();
        assert_eq!(r, Value::Number(42.0));
    }

    #[test]
    fn class_static_field_undefined() {
        let r = eval("class C { static u = undefined; } C.u;").unwrap();
        assert_eq!(r, Value::Undefined);
    }

    #[test]
    fn class_static_field_expression() {
        let r = eval("class C { static x = 1 + 2 * 3; } C.x;").unwrap();
        assert_eq!(r, Value::Number(7.0));
    }

    #[test]
    fn class_static_field_multiple_fields() {
        let r = eval(
            "class C { \
               static a = 1; \
               static b = 2; \
               static c = 3; \
             } \
             C.a + C.b + C.c;",
        )
        .unwrap();
        assert_eq!(r, Value::Number(6.0));
    }

    #[test]
    fn class_static_field_shadows_static_method() {
        // Fields are checked before methods in eval_class_member.
        let r = eval(
            "class C { \
               static foo() { return 1; } \
               static foo = 42; \
             } \
             C.foo;",
        )
        .unwrap();
        assert_eq!(r, Value::Number(42.0));
    }

    // ─── Class static: combined scenarios ─────────────────────────────────────

    #[test]
    fn class_static_method_getter_field_combined() {
        // Static getter returns 5; method uses this.field (10) * 2 = 20.
        // Result: 10 + 5 + 20 = 35.
        let r = eval(
            "class C { \
               static field = 10; \
               static get prop() { return 5; } \
               static method() { return this.field * 2; } \
             } \
             C.field + C.prop + C.method();",
        )
        .unwrap();
        assert_eq!(r, Value::Number(35.0));
    }

    #[test]
    fn class_static_super_inherited_getter() {
        let r = eval(
            "class Base { static get x() { return 1; } } \
             class Child extends Base {} \
             Child.x;",
        )
        .unwrap();
        assert_eq!(r, Value::Number(1.0));
    }

    #[test]
    fn class_static_super_inherited_setter() {
        let r = eval(
            "class Base { static set x(v) { this._b = v; } static get x() { return this._b; } } \
             class Child extends Base {} \
             Child.x = 99; \
             Child.x;",
        )
        .unwrap();
        assert_eq!(r, Value::Number(99.0));
    }

    #[test]
    fn class_static_super_overrides_method() {
        let r = eval(
            "class Base { static foo() { return 1; } } \
             class Child extends Base { static foo() { return 2; } } \
             Child.foo();",
        )
        .unwrap();
        assert_eq!(r, Value::Number(2.0));
    }

    #[test]
    fn class_static_super_inherited_field() {
        let r = eval(
            "class Base { static x = 7; } \
             class Child extends Base {} \
             Child.x;",
        )
        .unwrap();
        assert_eq!(r, Value::Number(7.0));
    }

    #[test]
    fn class_static_length_vs_instance_length() {
        // Class constructor length is formal param count; instance length is separate.
        let r = eval("class C { constructor(a, b) {} } C.length === 2;").unwrap();
        assert_eq!(r, Value::Boolean(true));
    }

    // ─── Global object fallback ────────────────────────────────────────────────

    #[test]
    fn global_object_fallback_parse_int() {
        let r = eval("parseInt !== undefined;").unwrap();
        assert_eq!(r, Value::Boolean(true));
    }

    #[test]
    fn global_object_fallback_is_finite() {
        let r = eval("isFinite !== undefined;").unwrap();
        assert_eq!(r, Value::Boolean(true));
    }
}
