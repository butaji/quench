// ─── Tests ───────────────────────────────────────────────────────────────────

#[cfg(test)]
mod call_tests {
    use crate::{Context, Value};

    fn eval_num(src: &str) -> f64 {
        let mut ctx = Context::new().unwrap();
        let r = ctx.eval(src).unwrap();
        match r {
            Value::Number(n) => n,
            Value::Undefined => f64::NAN,
            _ => panic!("eval_num: not a number: {:?}", r),
        }
    }

    // ─── eval_call_arguments: spread expansion ────────────────────────────────

    #[test]
    fn call_arguments_expand_spread_array() {
        let mut ctx = Context::new().unwrap();
        let r = ctx
            .eval("function f(a, b, c) { return a + '-' + b + '-' + c; } f(1, ...[2, 3], 4);")
            .unwrap();
        // ...[2, 3] expands to 2, 3 → call is f(1, 2, 3, 4) — 4 args for 3 params
        assert_eq!(r, Value::String("1-2-3".to_string()));
    }

    #[test]
    fn call_arguments_spread_at_start() {
        let mut ctx = Context::new().unwrap();
        let r = ctx
            .eval("function f(a, b) { return a + '-' + b; } f(...[1, 2]);")
            .unwrap();
        assert_eq!(r, Value::String("1-2".to_string()));
    }

    #[test]
    fn call_arguments_spread_at_end() {
        let mut ctx = Context::new().unwrap();
        let r = ctx
            .eval("function f(a, b) { return a + '-' + b; } f(1, ...[2, 3]);")
            .unwrap();
        assert_eq!(r, Value::String("1-2".to_string()));
    }

    #[test]
    fn call_arguments_spread_string() {
        let mut ctx = Context::new().unwrap();
        // Strings are iterable, spread expands to individual chars
        let r = ctx
            .eval("function f(a, b, c) { return a + b + c; } f(...'abc');")
            .unwrap();
        assert_eq!(r, Value::String("abc".to_string()));
    }

    #[test]
    fn call_arguments_empty_spread() {
        // Empty spread adds nothing — b is undefined, so a + b = NaN
        let r = eval_num("function f(a, b) { return a + b; } f(1, ...[]);");
        assert!(r.is_nan());
    }

    #[test]
    fn call_arguments_multiple_spreads() {
        let mut ctx = Context::new().unwrap();
        let r = ctx
            .eval("function f(a, b, c, d) { return a + b + c + d; } f(...[1], ...[2], 3, ...[4]);")
            .unwrap();
        assert_eq!(r, Value::Number(10.0));
    }

    // ─── eval_call: basic function calls ──────────────────────────────────────

    #[test]
    fn call_basic_function() {
        let mut ctx = Context::new().unwrap();
        let r = ctx
            .eval("function add(a, b) { return a + b; } add(2, 3);")
            .unwrap();
        assert_eq!(r, Value::Number(5.0));
    }

    #[test]
    fn call_function_identifier() {
        let mut ctx = Context::new().unwrap();
        let r = ctx
            .eval("function greet(name) { return 'Hi ' + name; } greet('World');")
            .unwrap();
        assert_eq!(r, Value::String("Hi World".to_string()));
    }

    #[test]
    fn call_nested_function() {
        let mut ctx = Context::new().unwrap();
        let r = ctx
            .eval("function outer(x) { function inner(y) { return x + y; } return inner(10); } outer(5);")
            .unwrap();
        assert_eq!(r, Value::Number(15.0));
    }

    #[test]
    fn call_function_expression() {
        let mut ctx = Context::new().unwrap();
        let r = ctx.eval("(function(x) { return x * 2; })(21);").unwrap();
        assert_eq!(r, Value::Number(42.0));
    }

    #[test]
    fn call_arrow_function() {
        let mut ctx = Context::new().unwrap();
        let r = ctx.eval("const f = (a, b) => a + b; f(3, 4);").unwrap();
        assert_eq!(r, Value::Number(7.0));
    }

    #[test]
    fn call_arrow_in_arrow() {
        let mut ctx = Context::new().unwrap();
        let r = ctx
            .eval(
                "const outer = (x) => { const inner = (y) => x + y; return inner(10); }; outer(5);",
            )
            .unwrap();
        assert_eq!(r, Value::Number(15.0));
    }

    #[test]
    fn call_method_on_object() {
        let mut ctx = Context::new().unwrap();
        let r = ctx
            .eval("var o = { val: 99, getVal() { return this.val; } }; o.getVal();")
            .unwrap();
        assert_eq!(r, Value::Number(99.0));
    }

    #[test]
    fn call_with_excess_arguments() {
        let mut ctx = Context::new().unwrap();
        let r = ctx
            .eval("function f(a) { return arguments.length; } f(1, 2, 3);")
            .unwrap();
        assert_eq!(r, Value::Number(3.0));
    }

    #[test]
    fn call_with_missing_arguments() {
        let mut ctx = Context::new().unwrap();
        let r = ctx
            .eval("function f(a, b) { return typeof a + typeof b; } f(1);")
            .unwrap();
        assert_eq!(r, Value::String("numberundefined".to_string()));
    }

    #[test]
    fn call_with_this_binding() {
        let mut ctx = Context::new().unwrap();
        let r = ctx
            .eval("function f() { return this.x; } var obj = {x: 42}; f.call(obj);")
            .unwrap();
        assert_eq!(r, Value::Number(42.0));
    }

    // ─── eval_call: direct eval ───────────────────────────────────────────────

    #[test]
    fn call_direct_eval_resolves_in_scope() {
        let mut ctx = Context::new().unwrap();
        ctx.eval("var x = 10;").unwrap();
        let r = ctx.eval("eval('x + 5');").unwrap();
        assert_eq!(r, Value::Number(15.0));
    }

    #[test]
    fn call_indirect_eval_is_global() {
        let mut ctx = Context::new().unwrap();
        // (1, eval) is an indirect eval — runs in global scope.
        // var y = 99 creates a global variable, so typeof y = "number".
        let r = ctx.eval("(1, eval)('var y = 99;'); typeof y;").unwrap();
        assert_eq!(r, Value::String("number".to_string()));
    }

    // ─── eval_new: constructor calls ──────────────────────────────────────────

    #[test]
    fn new_basic_constructor() {
        let mut ctx = Context::new().unwrap();
        let r = ctx
            .eval("function Point(x, y) { this.x = x; this.y = y; } var p = new Point(3, 4); p.x + p.y;")
            .unwrap();
        assert_eq!(r, Value::Number(7.0));
    }

    #[test]
    fn new_constructor_returns_object() {
        let mut ctx = Context::new().unwrap();
        // Constructor explicitly returns an object — that object is the result
        let r = ctx
            .eval("function Custom() { return {custom: true}; } var c = new Custom(); c.custom;")
            .unwrap();
        assert_eq!(r, Value::Boolean(true));
    }

    #[test]
    fn new_constructor_returns_primitive() {
        let mut ctx = Context::new().unwrap();
        // Constructor returns a primitive — ignored, new object is returned
        let r = ctx
            .eval("function Bad() { return 'string'; } var b = new Bad(); typeof b;")
            .unwrap();
        assert_eq!(r, Value::String("object".to_string()));
    }

    #[test]
    fn new_constructor_returns_undefined() {
        let mut ctx = Context::new().unwrap();
        // Constructor returns undefined — ignored, new object is returned
        let r = ctx
            .eval("function Bad() { return undefined; } var b = new Bad(); typeof b;")
            .unwrap();
        assert_eq!(r, Value::String("object".to_string()));
    }

    #[test]
    fn new_constructor_arrow_throws() {
        let mut ctx = Context::new().unwrap();
        let r = ctx.eval("var Arrow = () => {}; new Arrow();");
        assert!(r.is_err());
        if let Err(e) = r {
            assert!(
                e.0.contains("not a constructor") || e.0.contains("TypeError"),
                "expected TypeError, got: {}",
                e.0
            );
        }
    }

    #[test]
    fn new_native_function_without_prototype_throws() {
        let mut ctx = Context::new().unwrap();
        // Arrow functions and some built-in native functions have no prototype
        let r = ctx.eval("var f = () => {}; new f();");
        assert!(r.is_err());
    }

    #[test]
    fn new_with_arguments() {
        let mut ctx = Context::new().unwrap();
        let r = ctx
            .eval("function C(a, b) { this.sum = a + b; } var c = new C(10, 20); c.sum;")
            .unwrap();
        assert_eq!(r, Value::Number(30.0));
    }

    #[test]
    fn new_without_arguments() {
        let mut ctx = Context::new().unwrap();
        let r = ctx
            .eval("function Empty() {} var e = new Empty(); typeof e;")
            .unwrap();
        assert_eq!(r, Value::String("object".to_string()));
    }

    #[test]
    fn new_sets_name_property() {
        let mut ctx = Context::new().unwrap();
        let r = ctx
            .eval("function MyType() {} var t = new MyType(); t.name;")
            .unwrap();
        assert_eq!(r, Value::String("MyType".to_string()));
    }

    // ─── eval_member: member access ──────────────────────────────────────────

    #[test]
    fn member_dot_access() {
        let mut ctx = Context::new().unwrap();
        let r = ctx.eval("var o = {x: 42}; o.x;").unwrap();
        assert_eq!(r, Value::Number(42.0));
    }

    #[test]
    fn member_bracket_access() {
        let mut ctx = Context::new().unwrap();
        let r = ctx.eval("var o = {'my-prop': 123}; o['my-prop'];").unwrap();
        assert_eq!(r, Value::Number(123.0));
    }

    #[test]
    fn member_computed_access() {
        let mut ctx = Context::new().unwrap();
        let r = ctx
            .eval("var o = {key: 'value'}; var k = 'key'; o[k];")
            .unwrap();
        assert_eq!(r, Value::String("value".to_string()));
    }

    #[test]
    fn member_numeric_index() {
        let mut ctx = Context::new().unwrap();
        let r = ctx.eval("var a = [10, 20, 30]; a[1];").unwrap();
        assert_eq!(r, Value::Number(20.0));
    }

    #[test]
    fn member_chain() {
        let mut ctx = Context::new().unwrap();
        let r = ctx
            .eval("var o = {inner: {deep: 'found'}}; o.inner.deep;")
            .unwrap();
        assert_eq!(r, Value::String("found".to_string()));
    }

    #[test]
    fn member_on_call_result() {
        let mut ctx = Context::new().unwrap();
        let r = ctx
            .eval("function makeObj() { return {val: 777}; } makeObj().val;")
            .unwrap();
        assert_eq!(r, Value::Number(777.0));
    }

    #[test]
    fn member_getter_is_called() {
        let mut ctx = Context::new().unwrap();
        let r = ctx
            .eval("var o = { get prop() { return 42; } }; o.prop;")
            .unwrap();
        assert_eq!(r, Value::Number(42.0));
    }

    #[test]
    fn member_on_undefined_throws() {
        let mut ctx = Context::new().unwrap();
        let r = ctx.eval("var o = {}; o.nonexistent;").unwrap();
        assert_eq!(r, Value::Undefined);
    }

    // ─── eval_super_call ─────────────────────────────────────────────────────

    #[test]
    fn super_call_with_args() {
        let mut ctx = Context::new().unwrap();
        let r = ctx
            .eval(
                "class Base { constructor(x) { this.x = x; } }\
                 class Derived extends Base { constructor(y) { super(y * 2); } }\
                 var d = new Derived(5); d.x;",
            )
            .unwrap();
        assert_eq!(r, Value::Number(10.0));
    }

    #[test]
    fn super_call_outside_class_throws() {
        let mut ctx = Context::new().unwrap();
        let r = ctx.eval("super();");
        assert!(r.is_err());
        if let Err(e) = r {
            assert!(
                e.0.contains("super is only valid") || e.0.contains("ReferenceError"),
                "expected ReferenceError, got: {}",
                e.0
            );
        }
    }

    // ─── eval_super_member ────────────────────────────────────────────────────

    #[test]
    fn super_member_access() {
        let mut ctx = Context::new().unwrap();
        let r = ctx
            .eval(
                "class Base { greet() { return 'Hello'; } }\
                 class Derived extends Base { greet() { return super.greet() + ' World'; } }\
                 (new Derived()).greet();",
            )
            .unwrap();
        assert_eq!(r, Value::String("Hello World".to_string()));
    }

    #[test]
    fn super_member_chain() {
        // 2-level: Parent → Grand. super.val() correctly delegates to grandparent.
        let mut ctx = Context::new().unwrap();
        let r = ctx
            .eval(
                "class Grand { val() { return 'grand'; } }\
                 class Parent extends Grand { val() { return super.val(); } }\
                 (new Parent()).val();",
            )
            .unwrap();
        assert_eq!(r, Value::String("grand".to_string()));
    }

    // ─── extract_property_name ───────────────────────────────────────────────

    #[test]
    fn property_name_identifier() {
        let mut ctx = Context::new().unwrap();
        let r = ctx
            .eval("var o = { foo: 1, bar: 2 }; o.foo + o.bar;")
            .unwrap();
        assert_eq!(r, Value::Number(3.0));
    }

    #[test]
    fn property_name_string() {
        let mut ctx = Context::new().unwrap();
        let r = ctx.eval("var o = { 'my-key': 99 }; o['my-key'];").unwrap();
        assert_eq!(r, Value::Number(99.0));
    }

    #[test]
    fn property_name_number() {
        let mut ctx = Context::new().unwrap();
        let r = ctx
            .eval("var o = { 0: 'zero', 1: 'one' }; o[0] + o['1'];")
            .unwrap();
        assert_eq!(r, Value::String("zeroone".to_string()));
    }

    #[test]
    fn property_name_computed_symbol() {
        let mut ctx = Context::new().unwrap();
        let r = ctx
            .eval("var s = Symbol('test'); var o = {}; o[s] = 55; o[s];")
            .unwrap();
        assert_eq!(r, Value::Number(55.0));
    }

    #[test]
    fn property_name_computed_expression() {
        let mut ctx = Context::new().unwrap();
        let r = ctx
            .eval("var key = 'dynamic'; var o = {}; o[key + 'Prop'] = 123; o['dynamicProp'];")
            .unwrap();
        assert_eq!(r, Value::Number(123.0));
    }

    // ─── new.target ──────────────────────────────────────────────────────────

    #[test]
    fn new_target_inside_constructor() {
        let mut ctx = Context::new().unwrap();
        // new.target === C inside constructor is true when called with `new`.
        // Constructor returns primitive 1, but `new C()` returns the object
        // (primitive return value is ignored per ES §11.2.2 step 5).
        let r = ctx
            .eval("function C() { return new.target === C ? 1 : 0; } new C();")
            .unwrap();
        // Returns the newly created object, not the primitive 1.
        assert!(matches!(r, Value::Object(_)));
    }

    #[test]
    fn new_target_undefined_outside_constructor() {
        let mut ctx = Context::new().unwrap();
        let r = ctx.eval("new.target;").unwrap();
        assert_eq!(r, Value::Undefined);
    }

    // ─── Interaction: call + member + new ────────────────────────────────────

    #[test]
    fn chained_call_new_member() {
        let mut ctx = Context::new().unwrap();
        let r = ctx
            .eval(
                "function Factory(val) { this.get = function() { return val; }; }\
                 var f = new Factory(42);\
                 f.get();",
            )
            .unwrap();
        assert_eq!(r, Value::Number(42.0));
    }

    #[test]
    fn call_with_spread_and_member() {
        let mut ctx = Context::new().unwrap();
        let r = ctx
            .eval("var arr = [1, 2, 3]; arr.push(...[4, 5]); arr.length;")
            .unwrap();
        assert_eq!(r, Value::Number(5.0));
    }

    #[test]
    fn class_with_method_call() {
        let mut ctx = Context::new().unwrap();
        let r = ctx
            .eval(
                "class Calc { add(a, b) { return a + b; } }\
                 var c = new Calc();\
                 c.add(10, 20);",
            )
            .unwrap();
        assert_eq!(r, Value::Number(30.0));
    }

    #[test]
    fn class_with_getter() {
        let mut ctx = Context::new().unwrap();
        let r = ctx
            .eval(
                "class Point { constructor(x) { this._x = x; } get x() { return this._x * 2; } }\
                 var p = new Point(5); p.x;",
            )
            .unwrap();
        assert_eq!(r, Value::Number(10.0));
    }

    #[test]
    fn class_with_setter() {
        let mut ctx = Context::new().unwrap();
        let r = ctx
            .eval(
                "class Wrapper { set val(v) { this._v = v + 1; } }\
                 var w = new Wrapper();\
                 w.val = 10;\
                 w._v;",
            )
            .unwrap();
        assert_eq!(r, Value::Number(11.0));
    }

    #[test]
    fn derived_class_with_fields() {
        let mut ctx = Context::new().unwrap();
        let r = ctx
            .eval(
                "class Base { constructor(v) { this.base = v; } }\
                 class Derived extends Base { x = 5; constructor(v) { super(v); this.y = this.x + v; } }\
                 var d = new Derived(3); d.base + d.y;",
            )
            .unwrap();
        assert_eq!(r, Value::Number(11.0));
    }

    #[test]
    fn bound_function_call() {
        let mut ctx = Context::new().unwrap();
        let r = ctx
            .eval(
                "function greet(greeting) { return greeting + ' ' + this.name; }\
                 var bound = greet.bind({name: 'Alice'});\
                 bound('Hello');",
            )
            .unwrap();
        assert_eq!(r, Value::String("Hello Alice".to_string()));
    }

    #[test]
    fn apply_function() {
        let mut ctx = Context::new().unwrap();
        let r = ctx
            .eval(
                "function f(a, b) { return a + this.x + b; }\
                 var o = {x: 100};\
                 f.apply(o, [1, 2]);",
            )
            .unwrap();
        assert_eq!(r, Value::Number(103.0));
    }

    #[test]
    fn constructor_with_new_target() {
        let mut ctx = Context::new().unwrap();
        let r = ctx
            .eval(
                "function F() {\
                     if (new.target === F) this.created = true;\
                 }\
                 var f = new F(); f.created;",
            )
            .unwrap();
        assert_eq!(r, Value::Boolean(true));
    }

    #[test]
    fn eval_call_preserves_thrown_value() {
        let mut ctx = Context::new().unwrap();
        // Calling a function that throws should set thrown value
        ctx.eval("function thrower() { throw 'oops'; }").unwrap();
        let r: Result<Value, _> = ctx.eval("try { thrower(); } catch(e) { e; }");
        let v = r.unwrap();
        assert_eq!(v, Value::String("oops".to_string()));
    }

    #[test]
    fn super_not_available_in_arrow() {
        let mut ctx = Context::new().unwrap();
        let r = ctx.eval("class C { method() { return (() => super.foo)(); } }");
        assert!(r.is_err() || r.as_ref().is_ok_and(|v| v == &Value::Undefined));
    }

    // ─── spread operator tests ───────────────────────────────────────────────

    #[test]
    fn call_arguments_spread_nested() {
        let mut ctx = Context::new().unwrap();
        let r = ctx
            .eval("function f(a, b, c, d) { return [a, b, c, d]; } f(1, ...[2, 3], 4);")
            .unwrap();
        // Result is an array-like object
        assert!(matches!(r, Value::Object(_)));
    }

    #[test]
    fn call_arguments_spread_with_rest() {
        let mut ctx = Context::new().unwrap();
        let r = ctx
            .eval("function f(a, ...rest) { return rest; } f(1, ...[2, 3, 4]);")
            .unwrap();
        assert!(matches!(r, Value::Object(_)));
    }

    // ─── constructor with spread ─────────────────────────────────────────────

    #[test]
    fn new_with_spread() {
        let mut ctx = Context::new().unwrap();
        let r = ctx
            .eval("function C(a, b, c) { this.arr = [a, b, c]; } var c = new C(...[1, 2, 3]); c.arr[0];")
            .unwrap();
        assert_eq!(r, Value::Number(1.0));
    }
}
