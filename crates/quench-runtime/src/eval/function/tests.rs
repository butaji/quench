//! Unit tests for function call operations.
//!
//! Tests cover all public and pub(crate) functions in the parent module:
//!   `call_value`, `call_value_with_this`, `call_value_impl`,
//!   `call_js_function_with_this`, `call_js_function_impl`,
//!   `call_js_function_impl_with_strict`, `call_native_function`.

use crate::value::object::{PromiseObjectData, PromiseState};
use crate::value::{JsError, NativeConstructor, NativeFunction, Object, ObjectKind, Value};
use crate::Context;
use std::cell::RefCell;
use std::rc::Rc;

// ─── call_value: basic function calls ─────────────────────────────────────

#[test]
fn call_value_simple_fn() {
    let mut ctx = Context::new().unwrap();
    let v = ctx.eval("function f() { return 42; } f()").unwrap();
    assert_eq!(v, Value::Number(42.0));
}

#[test]
fn call_value_with_args() {
    let mut ctx = Context::new().unwrap();
    let v = ctx
        .eval("function add(a, b) { return a + b; } add(3, 4)")
        .unwrap();
    assert_eq!(v, Value::Number(7.0));
}

#[test]
fn call_value_arrow_fn() {
    let mut ctx = Context::new().unwrap();
    let v = ctx.eval("var f = (a, b) => a + b; f(10, 20)").unwrap();
    assert_eq!(v, Value::Number(30.0));
}

#[test]
fn call_value_arrow_no_args() {
    let mut ctx = Context::new().unwrap();
    let v = ctx.eval("var f = () => 99; f()").unwrap();
    assert_eq!(v, Value::Number(99.0));
}

#[test]
fn call_value_return_undefined() {
    let mut ctx = Context::new().unwrap();
    let v = ctx.eval("function nop() {} nop()").unwrap();
    assert_eq!(v, Value::Undefined);
}

#[test]
fn call_value_default_param() {
    let mut ctx = Context::new().unwrap();
    let v = ctx.eval("function f(x = 5) { return x; } f()").unwrap();
    assert_eq!(v, Value::Number(5.0));
}

#[test]
fn call_value_default_param_overridden() {
    let mut ctx = Context::new().unwrap();
    let v = ctx.eval("function f(x = 5) { return x; } f(3)").unwrap();
    assert_eq!(v, Value::Number(3.0));
}

#[test]
fn call_value_rest_param() {
    let mut ctx = Context::new().unwrap();
    let v = ctx
        .eval("function f(a, ...rest) { return rest.length; } f(1, 2, 3, 4)")
        .unwrap();
    assert_eq!(v, Value::Number(3.0));
}

#[test]
fn call_value_rest_param_empty() {
    let mut ctx = Context::new().unwrap();
    let v = ctx
        .eval("function f(...rest) { return rest.length; } f()")
        .unwrap();
    assert_eq!(v, Value::Number(0.0));
}

// ─── call_value_with_this: explicit this binding ─────────────────────────

#[test]
fn sloppy_call_boxes_primitive_this() {
    let mut ctx = Context::new().unwrap();
    let res = ctx
        .eval("function bar() { return typeof this; } bar.call(1);")
        .unwrap();
    assert_eq!(res, Value::String("object".to_string()));
}

#[test]
fn strict_body_keeps_primitive_this() {
    let mut ctx = Context::new().unwrap();
    let res = ctx
        .eval("function foo() { 'use strict'; return typeof this; } foo.call(1);")
        .unwrap();
    assert_eq!(res, Value::String("number".to_string()));
}

#[test]
fn sloppy_call_object_this_passes_through() {
    let mut ctx = Context::new().unwrap();
    let res = ctx
        .eval("var o = { name: 'x' }; function bar() { return typeof this; } bar.call(o);")
        .unwrap();
    assert_eq!(res, Value::String("object".to_string()));
}

#[test]
fn strict_call_keeps_null_this() {
    let mut ctx = Context::new().unwrap();
    let res = ctx
        .eval("function f() { 'use strict'; return this === null; } f.call(null);")
        .unwrap();
    assert_eq!(res, Value::Boolean(true));
}

#[test]
fn strict_call_keeps_undefined_this() {
    let mut ctx = Context::new().unwrap();
    let res = ctx
        .eval("function f() { 'use strict'; return this === undefined; } f.call(undefined);")
        .unwrap();
    assert_eq!(res, Value::Boolean(true));
}

#[test]
fn method_call_sets_this() {
    let mut ctx = Context::new().unwrap();
    let v = ctx
        .eval("var o = { x: 42, f() { return this.x; } }; o.f()")
        .unwrap();
    assert_eq!(v, Value::Number(42.0));
}

#[test]
fn call_value_apply_this() {
    let mut ctx = Context::new().unwrap();
    let v = ctx
        .eval("function f() { return this[0]; } f.apply([99])")
        .unwrap();
    assert_eq!(v, Value::Number(99.0));
}

// ─── Generator function calls ────────────────────────────────────────────

#[test]
fn generator_call_returns_object_with_next() {
    let mut ctx = Context::new().unwrap();
    let v = ctx
        .eval(
            "function* g() { yield 1; return 2; }
             var gen = g();
             typeof gen === 'object' && typeof gen.next === 'function'",
        )
        .unwrap();
    assert_eq!(v, Value::Boolean(true));
}

#[test]
fn generator_body_not_executed_on_call() {
    let mut ctx = Context::new().unwrap();
    let v = ctx
        .eval(
            "var executed = false;
             function* g() { executed = true; }
             var gen = g();
             executed",
        )
        .unwrap();
    assert_eq!(v, Value::Boolean(false));
}

#[test]
fn generator_yields_expected_values() {
    let mut ctx = Context::new().unwrap();
    let v = ctx
        .eval(
            "function* g() { yield 10; yield 20; }
             var gen = g();
             var a = gen.next().value;
             var b = gen.next().value;
             a + b",
        )
        .unwrap();
    assert_eq!(v, Value::Number(30.0));
}

#[test]
fn generator_next_returns_object() {
    let mut ctx = Context::new().unwrap();
    let v = ctx
        .eval(
            "function* g() { yield 7; }
             var gen = g();
             var r = gen.next();
             r.value === 7 && r.done === false",
        )
        .unwrap();
    assert_eq!(v, Value::Boolean(true));
}

// ─── Async function calls ────────────────────────────────────────────────

#[test]
fn async_call_returns_promise_object() {
    let mut ctx = Context::new().unwrap();
    let v = ctx
        .eval(
            "async function f() { return 42; }
             var p = f();
             typeof p === 'object' && p !== null && typeof p.then === 'function'",
        )
        .unwrap();
    assert_eq!(v, Value::Boolean(true));
}

#[test]
fn sync_function_returning_async_call_returns_promise() {
    let mut ctx = Context::new().unwrap();
    let empty = ctx
        .eval("function g() { return (async function() {})(); } typeof g()")
        .unwrap();
    assert_eq!(
        empty,
        Value::String("object".into()),
        "return (async fn)() must yield a Promise, not the resolved value"
    );
    let with_val = ctx
        .eval("function g() { return (async function() { return 42; })(); } typeof g()")
        .unwrap();
    assert_eq!(
        with_val,
        Value::String("object".into()),
        "return (async fn returning 42)() must yield a Promise"
    );
}

#[test]
fn async_call_is_not_value_directly() {
    let mut ctx = Context::new().unwrap();
    let v = ctx
        .eval("async function f() { return 42; } var p = f(); p === 42")
        .unwrap();
    assert_eq!(v, Value::Boolean(false));
}

#[test]
fn async_throw_rejected_promise() {
    let mut ctx = Context::new().unwrap();
    let v = ctx
        .eval(
            "async function f() { throw 99; }
             var p = f();
             typeof p === 'object' && typeof p.then === 'function'",
        )
        .unwrap();
    assert_eq!(v, Value::Boolean(true));
}

#[test]
fn sync_throwing_default_param_throws() {
    let mut ctx = Context::new().unwrap();
    // Test 1: arrow IIFE as default
    let r1 = ctx
        .eval(
            "function f1(_ = (() => { throw new RangeError('x'); })()) { return 'body ran'; }
         try { f1(); 'r1:resolved' } catch(e) { 'r1:rejected:' + e.constructor.name }",
        )
        .unwrap();
    assert_eq!(r1, Value::String("r1:rejected:RangeError".into()));

    // Test 2: named function call in default
    let r2 = ctx
        .eval(
            "function thrower() { throw new RangeError('x'); }
         function f2(_ = thrower()) { return 'body ran'; }
         try { f2(); 'r2:resolved' } catch(e) { 'r2:rejected:' + e.constructor.name }",
        )
        .unwrap();
    assert_eq!(r2, Value::String("r2:rejected:RangeError".into()));

    // Test 3: standalone IIFE
    let r3 = ctx.eval(
        "try { (function() { throw new RangeError('x'); })(); 'r3:no throw' } catch(e) { 'r3:' + e.name }"
    ).unwrap();
    assert_eq!(r3, Value::String("r3:RangeError".into()));

    // Test 4: just throw in default
    let r4 = ctx
        .eval(
            "function f4(_ = (1+2)) { return _; }
         try { f4(); 'r4:resolved:' + _ } catch(e) { 'r4:rejected' }",
        )
        .unwrap();
    assert_eq!(r4, Value::String("r4:rejected".into()));

    // Test 5: evaluate default directly
    let r5 = ctx.eval("function f5(_ = 42) { return _; } f5()").unwrap();
    assert_eq!(r5, Value::Number(42.0));

    // Test 6: call f with explicit undefined
    let r6 = ctx
        .eval("function f6(_ = 99) { return _; } f6(undefined)")
        .unwrap();
    assert_eq!(r6, Value::Number(99.0));
}

#[test]
fn async_throwing_default_param_rejects_promise() {
    let mut ctx = Context::new().unwrap();
    let p = ctx
        .eval(
            "async function f(_ = (function() { throw new RangeError('x'); }())) {
               return 'body ran';
             }
             f()",
        )
        .unwrap();
    // Result should be a Promise object
    match &p {
        Value::Object(obj) => {
            let obj_ref = obj.borrow();
            if let Some(ref data) = obj_ref.promise_data {
                assert_eq!(
                    data.state,
                    PromiseState::Rejected,
                    "Promise should be rejected when default param throws, got state {:?}",
                    data.state
                );
                // Rejection reason should be the actual error object, not a string
                match &data.result {
                    Value::Object(err_obj) => {
                        let err_ref = err_obj.borrow();
                        match err_ref.get("name") {
                            Some(Value::String(name)) => {
                                assert_eq!(
                                    name, "RangeError",
                                    "Rejection reason should be a RangeError object"
                                );
                            }
                            other => panic!(
                                "Rejection reason should have .name property, got: {:?}",
                                other
                            ),
                        }
                    }
                    other => panic!(
                        "Rejection reason should be an Error object, got: {:?}",
                        other
                    ),
                }
            } else {
                panic!("Not a promise object (no promise_data)");
            }
        }
        other => panic!("Expected Promise object, got: {:?}", other),
    }
}

// ─── NativeFunction calls (via JS) ───────────────────────────────────────

#[test]
fn native_fn_parse_int() {
    let mut ctx = Context::new().unwrap();
    let v = ctx.eval("parseInt('42')").unwrap();
    assert_eq!(v, Value::Number(42.0));
}

#[test]
fn native_fn_math_sin() {
    let mut ctx = Context::new().unwrap();
    let v = ctx.eval("Math.sin(0)").unwrap();
    assert_eq!(v, Value::Number(0.0));
}

#[test]
fn native_fn_is_array() {
    let mut ctx = Context::new().unwrap();
    let v = ctx.eval("Array.isArray([])").unwrap();
    assert_eq!(v, Value::Boolean(true));
}

#[test]
fn native_fn_is_array_false() {
    let mut ctx = Context::new().unwrap();
    let v = ctx.eval("Array.isArray({})").unwrap();
    assert_eq!(v, Value::Boolean(false));
}

// ─── NativeConstructor calls (as functions, not 'new') ───────────────────

#[test]
fn native_constructor_number_as_fn() {
    let mut ctx = Context::new().unwrap();
    let v = ctx.eval("Number('42')").unwrap();
    assert_eq!(v, Value::Number(42.0));
}

#[test]
fn native_constructor_boolean_as_fn() {
    let mut ctx = Context::new().unwrap();
    let v = ctx.eval("Boolean(1)").unwrap();
    assert_eq!(v, Value::Boolean(true));
}

#[test]
fn native_constructor_string_as_fn() {
    let mut ctx = Context::new().unwrap();
    let v = ctx.eval("String(42)").unwrap();
    assert_eq!(v, Value::String("42".to_string()));
}

#[test]
fn native_constructor_number_as_fn_invalid() {
    let mut ctx = Context::new().unwrap();
    let v = ctx.eval("Number('xyz')").unwrap();
    match v {
        Value::Number(n) => assert!(n.is_nan(), "expected NaN"),
        _ => panic!("expected Number, got {:?}", v),
    }
}

// ─── Class calls ─────────────────────────────────────────────────────────

#[test]
fn class_new_creates_instance() {
    let mut ctx = Context::new().unwrap();
    let v = ctx
        .eval("class C { constructor(x) { this.x = x; } } new C(42).x")
        .unwrap();
    assert_eq!(v, Value::Number(42.0));
}

#[test]
fn class_super_constructor_call() {
    let mut ctx = Context::new().unwrap();
    let v = ctx
        .eval(
            "class Base { constructor(v) { this.baseVal = v; } }
             class Derived extends Base { constructor(v) { super(v); this.derived = true; } }
             var d = new Derived(99);
             d.baseVal === 99 && d.derived === true",
        )
        .unwrap();
    assert_eq!(v, Value::Boolean(true));
}

#[test]
fn class_instanceof() {
    let mut ctx = Context::new().unwrap();
    let v = ctx
        .eval("class C {} var c = new C(); c instanceof C")
        .unwrap();
    assert_eq!(v, Value::Boolean(true));
}

// ─── Error cases: calling non-function values ────────────────────────────

#[test]
fn call_null_throws_typeerror() {
    let mut ctx = Context::new().unwrap();
    let v = ctx
        .eval(
            "var threw = false;
             try { null(); } catch(e) { threw = e instanceof TypeError; }
             threw",
        )
        .unwrap();
    assert_eq!(v, Value::Boolean(true));
}

#[test]
fn call_undefined_throws_typeerror() {
    let mut ctx = Context::new().unwrap();
    let v = ctx
        .eval(
            "var threw = false;
             try { undefined(); } catch(e) { threw = e instanceof TypeError; }
             threw",
        )
        .unwrap();
    assert_eq!(v, Value::Boolean(true));
}

#[test]
fn call_number_throws_typeerror() {
    let mut ctx = Context::new().unwrap();
    let v = ctx
        .eval(
            "var threw = false;
             try { 42(); } catch(e) { threw = e instanceof TypeError; }
             threw",
        )
        .unwrap();
    assert_eq!(v, Value::Boolean(true));
}

#[test]
fn call_string_throws_typeerror() {
    let mut ctx = Context::new().unwrap();
    let v = ctx
        .eval(
            "var threw = false;
             try { 'hello'(); } catch(e) { threw = e instanceof TypeError; }
             threw",
        )
        .unwrap();
    assert_eq!(v, Value::Boolean(true));
}

#[test]
fn call_bool_throws_typeerror() {
    let mut ctx = Context::new().unwrap();
    let v = ctx
        .eval(
            "var threw = false;
             try { true(); } catch(e) { threw = e instanceof TypeError; }
             threw",
        )
        .unwrap();
    assert_eq!(v, Value::Boolean(true));
}

#[test]
fn call_symbol_throws_typeerror() {
    let mut ctx = Context::new().unwrap();
    let v = ctx
        .eval(
            "var s = Symbol('x');
             var threw = false;
             try { s(); } catch(e) { threw = e instanceof TypeError; }
             threw",
        )
        .unwrap();
    assert_eq!(v, Value::Boolean(true));
}

#[test]
fn call_plain_object_throws_typeerror() {
    let mut ctx = Context::new().unwrap();
    // A null-prototype object has no callable constructor → TypeError.
    let v = ctx
        .eval(
            "var obj = Object.create(null);
             var threw = false;
             try { obj(); } catch(e) { threw = e instanceof TypeError; }
             threw",
        )
        .unwrap();
    assert_eq!(v, Value::Boolean(true));
}

// ─── Arrow function `this` binding ───────────────────────────────────────

#[test]
fn arrow_fn_inherits_outer_this() {
    let mut ctx = Context::new().unwrap();
    let v = ctx
        .eval(
            "var o = { x: 10, f: function() {
               var arrow = () => this.x;
               return arrow();
             }};
             o.f()",
        )
        .unwrap();
    assert_eq!(v, Value::Number(10.0));
}

#[test]
fn arrow_fn_this_not_overridden_by_call() {
    let mut ctx = Context::new().unwrap();
    let v = ctx
        .eval(
            "var o = { x: 10 };
             var arrow = () => this;
             arrow.call(o) === arrow",
        )
        .unwrap();
    // Arrow functions ignore .call()'s thisArg — they use lexical this.
    // In global scope, `this` is the global object. arrow.call(o) still
    // returns global.
    assert_eq!(v, Value::Boolean(false));
}

// ─── Direct Rust-level: call_value ─────────────────────────────────────

#[test]
fn call_value_direct_simple() {
    let mut ctx = Context::new().unwrap();
    // Construct a JS function via eval, then call it from Rust
    let v = ctx
        .eval("function double(x) { return x * 2; } double")
        .unwrap();

    let result = crate::eval::function::call_value(v, vec![Value::Number(7.0)]);
    assert_eq!(result.unwrap(), Value::Number(14.0));
}

#[test]
fn call_value_direct_no_args() {
    let mut ctx = Context::new().unwrap();
    let v = ctx.eval("function one() { return 1; } one").unwrap();

    let result = crate::eval::function::call_value(v, vec![]);
    assert_eq!(result.unwrap(), Value::Number(1.0));
}

#[test]
fn call_value_direct_non_function() {
    let result = crate::eval::function::call_value(Value::Null, vec![]);
    assert!(result.is_err());
}

// ─── Direct Rust-level: call_value_with_this ─────────────────────────────

#[test]
fn call_value_with_this_direct_strict() {
    let mut ctx = Context::new().unwrap();
    let v = ctx
        .eval("function f() { 'use strict'; return this; } f")
        .unwrap();

    let result = crate::eval::function::call_value_with_this(v, vec![], Value::Number(42.0));
    assert_eq!(result.unwrap(), Value::Number(42.0));
}

#[test]
fn call_value_with_this_direct_sloppy_boxes() {
    let mut ctx = Context::new().unwrap();
    let v = ctx.eval("function f() { return typeof this; } f").unwrap();

    let result = crate::eval::function::call_value_with_this(v, vec![], Value::Number(1.0));
    // Sloppy mode boxes primitive this → typeof becomes "object"
    assert_eq!(result.unwrap(), Value::String("object".to_string()));
}

// ─── Direct Rust-level: call_value_impl with force_strict ────────────────

#[test]
fn call_value_impl_force_strict_keeps_primitive_this() {
    let mut ctx = Context::new().unwrap();
    let v = ctx.eval("function f() { return typeof this; } f").unwrap();

    // force_strict=true makes even a sloppy function treat primitive
    // `this` as-is (no boxing).
    let result = crate::eval::function::call_value_impl(v, vec![], Value::Number(1.0), true);
    assert_eq!(result.unwrap(), Value::String("number".to_string()));
}

#[test]
fn call_value_impl_no_force_strict_boxes() {
    let mut ctx = Context::new().unwrap();
    let v = ctx.eval("function f() { return typeof this; } f").unwrap();

    // force_strict=false — default sloppy behavior boxes the primitive.
    let result = crate::eval::function::call_value_impl(v, vec![], Value::Number(1.0), false);
    assert_eq!(result.unwrap(), Value::String("object".to_string()));
}

// ─── Direct Rust-level: call_js_function_with_this ───────────────────────

#[test]
fn call_js_function_with_this_direct() {
    let mut ctx = Context::new().unwrap();
    let Value::Function(f) = ctx
        .eval("function f() { 'use strict'; return this; } f")
        .unwrap()
    else {
        panic!("expected a function");
    };

    let result = crate::eval::function::call_js_function_with_this(
        f,
        vec![],
        Value::String("hello".to_string()),
    );
    assert_eq!(result.unwrap(), Value::String("hello".to_string()));
}

// ─── Direct Rust-level: NativeFunction ───────────────────────────────────

#[test]
fn call_native_function_direct() {
    let nf = Rc::new(NativeFunction::new(|args: Vec<Value>| match args.first() {
        Some(Value::Number(n)) => Ok(Value::Number(n * 2.0)),
        _ => Err(JsError("expected a number".to_string())),
    }));
    let func = Value::NativeFunction(nf);

    let result = crate::eval::function::call_value(func, vec![Value::Number(5.0)]);
    assert_eq!(result.unwrap(), Value::Number(10.0));
}

#[test]
fn call_native_function_with_this() {
    let nf = Rc::new(NativeFunction::new(|args: Vec<Value>| {
        Ok(args.first().cloned().unwrap_or(Value::Undefined))
    }));
    let func = Value::NativeFunction(nf);

    let result = crate::eval::function::call_value_with_this(
        func,
        vec![Value::Number(99.0)],
        Value::String("ignored".to_string()),
    );
    assert_eq!(result.unwrap(), Value::Number(99.0));
}

#[test]
fn call_native_function_zero_args() {
    let nf = Rc::new(NativeFunction::new(|args: Vec<Value>| {
        Ok(Value::Number(args.len() as f64))
    }));
    let func = Value::NativeFunction(nf);

    let result = crate::eval::function::call_value(func, vec![]);
    assert_eq!(result.unwrap(), Value::Number(0.0));
}

// ─── Direct Rust-level: NativeConstructor ────────────────────────────────

#[test]
fn call_native_constructor_direct_as_fn() {
    let proto = Rc::new(RefCell::new(Object::new(ObjectKind::Ordinary)));
    let nc = Rc::new(NativeConstructor::new(
        |_args: Vec<Value>| Ok(Value::Number(42.0)),
        proto,
    ));
    let func = Value::NativeConstructor(nc);

    let result = crate::eval::function::call_value(func, vec![]);
    assert_eq!(result.unwrap(), Value::Number(42.0));
}

#[test]
fn call_native_constructor_with_args() {
    let proto = Rc::new(RefCell::new(Object::new(ObjectKind::Ordinary)));
    let nc = Rc::new(NativeConstructor::new(
        |args: Vec<Value>| {
            let sum: f64 = args
                .iter()
                .filter_map(|v| match v {
                    Value::Number(n) => Some(n),
                    _ => None,
                })
                .sum();
            Ok(Value::Number(sum))
        },
        proto,
    ));
    let func = Value::NativeConstructor(nc);

    let result = crate::eval::function::call_value(
        func,
        vec![
            Value::Number(10.0),
            Value::Number(20.0),
            Value::Number(30.0),
        ],
    );
    assert_eq!(result.unwrap(), Value::Number(60.0));
}

// ─── Direct Rust-level: call_native_function pub(crate) fn ────────────────

#[test]
fn call_native_function_direct_fn() {
    let nf = Rc::new(NativeFunction::new(|args: Vec<Value>| {
        Ok(Value::Number(args.len() as f64))
    }));

    let result =
        crate::eval::function::call_native_function(nf, vec![Value::Undefined], Value::Null);
    assert_eq!(result.unwrap(), Value::Number(1.0));
}

// ─── Generator via Rust-level direct call ────────────────────────────────

#[test]
fn generator_call_returns_generator_value_direct() {
    let mut ctx = Context::new().unwrap();
    let v = ctx.eval("function* g() { yield 1; } g").unwrap();

    let result = crate::eval::function::call_value(v, vec![]);
    let gen_val = result.unwrap();
    assert!(matches!(gen_val, Value::Generator(_)));
}

// ─── Async via Rust-level direct call ────────────────────────────────────

#[test]
fn async_call_returns_promise_value_direct() {
    let mut ctx = Context::new().unwrap();
    let v = ctx.eval("async function f() { return 1; } f").unwrap();

    let result = crate::eval::function::call_value(v, vec![]);
    let promise_val = result.unwrap();
    // A Promise is an object with a .then method
    match &promise_val {
        Value::Object(obj) => {
            let has_then = obj.borrow().get("then").is_some();
            assert!(has_then, "async call result should have .then");
        }
        _ => panic!("async call should return Value::Object (Promise)"),
    }
}

// ─── Class via Rust-level direct call (with this=Undefined) ──────────────

#[test]
fn class_call_without_new_throws_type_error() {
    let mut ctx = Context::new().unwrap();
    let v = ctx
        .eval("class C { constructor(x) { this.x = x; } } C")
        .unwrap();

    // ES spec §14.2.2: class constructors have no [[Call]] internal method.
    // Calling a class without `new` must throw TypeError.
    let result =
        crate::eval::function::call_value_with_this(v, vec![Value::Number(7.0)], Value::Undefined);
    match result {
        Err(JsError(msg)) => {
            assert!(msg.contains("TypeError"), "Expected TypeError, got: {msg}");
        }
        Ok(val) => panic!("Expected TypeError when calling class without new, got: {val:?}"),
    }
}

// ─── call_value_impl: non-function fallthrough ───────────────────────────

#[test]
fn call_value_impl_undefined_errs() {
    let result =
        crate::eval::function::call_value_impl(Value::Undefined, vec![], Value::Undefined, false);
    assert!(result.is_err());
}

#[test]
fn call_value_impl_null_errs() {
    let result =
        crate::eval::function::call_value_impl(Value::Null, vec![], Value::Undefined, false);
    assert!(result.is_err());
}

// ─── Tail Call Optimization ───────────────────────────────────────────────

/// TCO: simple tail-recursive function in strict mode.
#[test]
fn tco_simple_tail_call() {
    let mut ctx = Context::new().unwrap();
    let v = ctx
        .eval(
            r#""use strict";
            function f(n) {
              if (n === 0) return 42;
              return f(n - 1);
            }
            f(1000)"#,
        )
        .unwrap();
    assert_eq!(v, Value::Number(42.0));
}

/// TCO: simple tail-recursive function with large depth.
#[test]
fn tco_simple_tail_call_deep() {
    let mut ctx = Context::new().unwrap();
    let v = ctx
        .eval(
            r#""use strict";
            function f(n) {
              if (n === 0) return 42;
              return f(n - 1);
            }
            f(10000)"#,
        )
        .unwrap();
    assert_eq!(v, Value::Number(42.0));
}

/// TCO: verify trampoline is working with small depth first.
#[test]
fn tco_small_depth() {
    let mut ctx = Context::new().unwrap();
    let v = ctx
        .eval(
            r#""use strict";
            function f(n) {
              if (n === 0) return 'done';
              return f(n - 1);
            }
            f(5)"#,
        )
        .unwrap();
    assert_eq!(v, Value::String("done".into()));
}

/// TCO: medium depth test.
#[test]
fn tco_medium_depth() {
    let mut ctx = Context::new().unwrap();
    let v = ctx
        .eval(
            r#""use strict";
            function f(n) {
              if (n === 0) return 42;
              return f(n - 1);
            }
            f(5000)"#,
        )
        .unwrap();
    assert_eq!(v, Value::Number(42.0));
}

/// TCO: mutual tail recursion (two functions calling each other).
#[test]
fn tco_mutual_tail_recursion() {
    let mut ctx = Context::new().unwrap();
    let v = ctx
        .eval(
            r#""use strict";
            function even(n) {
              if (n === 0) return true;
              return odd(n - 1);
            }
            function odd(n) {
              if (n === 0) return false;
              return even(n - 1);
            }
            even(100000)"#,
        )
        .unwrap();
    assert_eq!(v, Value::Boolean(true));
}

/// TCO: tail call inside a block statement (per ES spec §14.2.1).
#[test]
fn tco_tail_call_in_block() {
    let mut ctx = Context::new().unwrap();
    let v = ctx
        .eval(
            r#""use strict";
            function f(n) {
              if (n === 0) return 'done';
              { return f(n - 1); }
            }
            f(100000)"#,
        )
        .unwrap();
    assert_eq!(v, Value::String("done".to_string()));
}

/// TCO: named function expression calling itself in tail position.
#[test]
fn tco_named_function_expression() {
    let mut ctx = Context::new().unwrap();
    let v = ctx
        .eval(
            r#""use strict";
            (function fact(n) {
              if (n <= 1) return 1;
              return n * fact(n - 1);
            })(10)"#,
        )
        .unwrap();
    assert_eq!(v, Value::Number(3628800.0));
}

/// TCO: non-tail call does NOT optimize (value must be preserved).
/// `return x;` after `f(n-1)` is NOT in tail position.
#[test]
fn non_tail_call_value_preserved() {
    let mut ctx = Context::new().unwrap();
    let v = ctx
        .eval(
            r#""use strict";
            function f(n) {
              if (n === 0) return 99;
              var x = f(n - 1);
              return x + 1;
            }
            f(5)"#,
        )
        .unwrap();
    // 5 non-tail calls + 1 base = 5 additions of 1 to 99 = 104
    assert_eq!(v, Value::Number(104.0));
}

/// TCO: depth=1: single tail-call, returns correctly.
#[test]
fn tco_depth_1() {
    let mut ctx = Context::new().unwrap();
    let v = ctx
        .eval(
            r#""use strict";
            function f() { return g();
            }
            function g() { return 42; }
            f()"#,
        )
        .unwrap();
    assert_eq!(v, Value::Number(42.0));
}

/// TCO: depth=2: two tail-calls, returns correctly.
#[test]
fn tco_depth_2() {
    let mut ctx = Context::new().unwrap();
    let v = ctx
        .eval(
            r#""use strict";
            function f() { return g();
            }
            function g() { return h();
            }
            function h() { return 42; }
            f()"#,
        )
        .unwrap();
    assert_eq!(v, Value::Number(42.0));
}

/// TCO: depth=2 with accumulator: f returns g returns 42, accumulator stays correct.
#[test]
fn tco_depth_2_accumulator() {
    let mut ctx = Context::new().unwrap();
    let v = ctx
        .eval(
            r#""use strict";
            function f() { return g(1);
            }
            function g(a) { return h(a + 1);
            }
            function h(b) { return b * 10; }
            f()"#,
        )
        .unwrap();
    assert_eq!(v, Value::Number(20.0));
}

/// TCO: depth=2 non-tail: f → g (tail), g returns value, f adds to it.
#[test]
fn tco_depth_2_nontail() {
    let mut ctx = Context::new().unwrap();
    let v = ctx
        .eval(
            r#""use strict";
            function f() { var x = g(); return x + 1; }
            function g() { return 10; }
            f()"#,
        )
        .unwrap();
    // g returns 10, f adds 1 = 11
    assert_eq!(v, Value::Number(11.0));
}

/// TCO: depth=3 non-tail: f → g (tail) → h (tail), h returns, g adds, f adds.
#[test]
fn tco_depth_3_nontail_chain() {
    let mut ctx = Context::new().unwrap();
    let v = ctx
        .eval(
            r#""use strict";
            function f() { var x = g(); return x + 1; }
            function g() { var y = h(); return y + 10; }
            function h() { return 100; }
            f()"#,
        )
        .unwrap();
    // h returns 100, g adds 10 = 110, f adds 1 = 111
    assert_eq!(v, Value::Number(111.0));
}

// ─── bind_params: direct Rust-level unit tests ─────────────────────────────

use crate::ast::Param;
use crate::env::Environment;
use crate::value::function::ValueFunction;

#[test]
fn bind_params_positional() {
    let closure = Rc::new(RefCell::new(Environment::new()));
    let func = ValueFunction::new(
        Some("test".to_string()),
        vec![Param::new("a"), Param::new("b")],
        vec![],
        Rc::clone(&closure),
        false,
        false,
    );
    let call_env = Rc::new(RefCell::new(Environment::new()));
    let params = vec![Param::new("a"), Param::new("b")];
    let args = vec![Value::Number(1.0), Value::Number(2.0)];

    crate::eval::function::bind_params(&func, &params, &args, &call_env).unwrap();

    assert_eq!(call_env.borrow().get("a"), Some(Value::Number(1.0)));
    assert_eq!(call_env.borrow().get("b"), Some(Value::Number(2.0)));
}

#[test]
fn bind_params_extra_args() {
    let closure = Rc::new(RefCell::new(Environment::new()));
    let func = ValueFunction::new(
        Some("test".to_string()),
        vec![Param::new("a")],
        vec![],
        Rc::clone(&closure),
        false,
        false,
    );
    let call_env = Rc::new(RefCell::new(Environment::new()));
    let params = vec![Param::new("a")];
    let args = vec![Value::Number(1.0), Value::Number(2.0), Value::Number(3.0)];

    crate::eval::function::bind_params(&func, &params, &args, &call_env).unwrap();

    assert_eq!(call_env.borrow().get("a"), Some(Value::Number(1.0)));
    // Extra args are ignored — no additional bindings created
    assert!(call_env.borrow().get("b").is_none());
}

#[test]
fn bind_params_missing_args() {
    let closure = Rc::new(RefCell::new(Environment::new()));
    let func = ValueFunction::new(
        Some("test".to_string()),
        vec![Param::new("a"), Param::new("b")],
        vec![],
        Rc::clone(&closure),
        false,
        false,
    );
    let call_env = Rc::new(RefCell::new(Environment::new()));
    let params = vec![Param::new("a"), Param::new("b")];
    let args = vec![Value::Number(1.0)]; // Only one arg, 'b' is missing

    crate::eval::function::bind_params(&func, &params, &args, &call_env).unwrap();

    assert_eq!(call_env.borrow().get("a"), Some(Value::Number(1.0)));
    assert_eq!(call_env.borrow().get("b"), Some(Value::Undefined));
}

#[test]
fn bind_params_arrow_no_this() {
    let closure = Rc::new(RefCell::new(Environment::new()));
    let mut func = ValueFunction::new(
        Some("arrow".to_string()),
        vec![Param::new("x")],
        vec![],
        Rc::clone(&closure),
        false,
        false,
    );
    func.is_arrow = true; // Mark as arrow function
    let call_env = Rc::new(RefCell::new(Environment::new()));

    let params = vec![Param::new("x")];
    let args = vec![Value::Number(42.0)];

    crate::eval::function::bind_params(&func, &params, &args, &call_env).unwrap();

    // Arrow function parameters are still bound correctly
    assert_eq!(call_env.borrow().get("x"), Some(Value::Number(42.0)));
}

#[test]
fn class_method_default_param_self_reference_throws_reference_error() {
    let mut ctx = Context::new().unwrap();
    crate::builtins::register_builtins(&mut ctx);
    let err = ctx
        .eval(
            "var x = 0; \
             var callCount = 0; \
             class C { method(x = x) { callCount = callCount + 1; } } \
             try { C.prototype.method(); 'ok'; } catch (e) { e.name + ':' + callCount; }",
        )
        .unwrap();
    assert_eq!(err, Value::String("ReferenceError:0".into()));
}

/// Per ES spec §10.2.2 [[Construct]]: plain objects (e.g. Math) have no
/// [[Construct]] — `new Math()` must throw TypeError.
#[test]
fn new_object_without_constructor_throws_typeerror() {
    let mut ctx = crate::Context::new().unwrap();
    crate::builtins::register_builtins(&mut ctx);
    let result = ctx.eval("try { new Math(); 'no throw'; } catch(e) { e.name + ':' + e.message; }");
    let v = result.unwrap();
    let s = match &v {
        Value::String(s) => s.as_str(),
        _ => panic!("expected String, got {:?}", v),
    };
    assert!(
        s.contains("TypeError"),
        "new Math() should throw TypeError, got: {}",
        s
    );
}

// ─── Stage 26 fixes: prototype chain, return values, hoisting, etc. ──────

#[test]
fn function_return_is_undefined_without_explicit_return() {
    let mut ctx = Context::new().unwrap();
    let v = ctx.eval("function f() { var x = 1; } f()").unwrap();
    assert_eq!(v, Value::Undefined);
}

#[test]
fn function_return_is_undefined_expression_last_stmt() {
    let mut ctx = Context::new().unwrap();
    let v = ctx.eval("function f() { x = true; } f()").unwrap();
    assert_eq!(v, Value::Undefined);
}

#[test]
fn function_return_is_undefined_sloppy_assign() {
    let mut ctx = Context::new().unwrap();
    // ES spec: without explicit return, function returns undefined
    let v = ctx.eval("function f() { var x = 1; x = 2; } f()").unwrap();
    assert_eq!(v, Value::Undefined);
}

#[test]
fn function_explicit_return_still_works() {
    let mut ctx = Context::new().unwrap();
    let v = ctx.eval("function f() { return 42; } f()").unwrap();
    assert_eq!(v, Value::Number(42.0));
}

#[test]
fn function_explicit_return_empty() {
    let mut ctx = Context::new().unwrap();
    let v = ctx.eval("function f() { return; } f()").unwrap();
    assert_eq!(v, Value::Undefined);
}

#[test]
fn function_explicit_return_undefined() {
    let mut ctx = Context::new().unwrap();
    let v = ctx.eval("function f() { return undefined; } f()").unwrap();
    assert_eq!(v, Value::Undefined);
}

#[test]
fn arguments_is_defined_in_function() {
    let mut ctx = Context::new().unwrap();
    let v = ctx
        .eval("function f() { return typeof arguments; } f()")
        .unwrap();
    assert_eq!(v, Value::String("object".to_string()));
}

#[test]
fn arguments_delete_returns_false() {
    let mut ctx = Context::new().unwrap();
    let v = ctx
        .eval("function f() { return delete arguments; } f()")
        .unwrap();
    assert_eq!(v, Value::Boolean(false));
}

#[test]
fn arguments_not_removed_by_delete() {
    let mut ctx = Context::new().unwrap();
    let v = ctx
        .eval("function f() { delete arguments; return typeof arguments; } f()")
        .unwrap();
    assert_eq!(v, Value::String("object".to_string()));
}

#[test]
fn delete_function_decl_returns_false() {
    let mut ctx = Context::new().unwrap();
    let v = ctx.eval("function f(){} var r = delete f; r").unwrap();
    assert_eq!(v, Value::Boolean(false));
}

#[test]
fn delete_function_decl_can_still_call_after_delete() {
    let mut ctx = Context::new().unwrap();
    let v = ctx.eval("function f(){ return 1; } delete f; f()").unwrap();
    assert_eq!(v, Value::Number(1.0));
}

#[test]
fn function_decl_hoisted_over_var() {
    let mut ctx = Context::new().unwrap();
    let v = ctx
        .eval("function f(){ return 1; } var f = 2; typeof f === 'number'")
        .unwrap();
    assert_eq!(v, Value::Boolean(true));
}

#[test]
fn function_decl_before_var_in_global() {
    let mut ctx = Context::new().unwrap();
    let v = ctx.eval("function f(){} typeof f === 'function'").unwrap();
    assert_eq!(v, Value::Boolean(true));
}

#[test]
fn strict_function_caller_set_throws_typeerror() {
    let mut ctx = Context::new().unwrap();
    let v = ctx
        .eval(
            "function f() { 'use strict'; }
             try { f.caller = 1; 'no throw'; } catch(e) { e.name; }",
        )
        .unwrap();
    assert_eq!(v, Value::String("TypeError".to_string()));
}

#[test]
fn strict_function_arguments_set_throws_typeerror() {
    let mut ctx = Context::new().unwrap();
    let v = ctx
        .eval(
            "function f() { 'use strict'; }
             try { f.arguments = 1; 'no throw'; } catch(e) { e.name; }",
        )
        .unwrap();
    assert_eq!(v, Value::String("TypeError".to_string()));
}

#[test]
fn strict_function_caller_get_throws_typeerror() {
    let mut ctx = Context::new().unwrap();
    let v = ctx
        .eval(
            "function f() { 'use strict'; }
             try { var x = f.caller; 'no throw'; } catch(e) { e.name; }",
        )
        .unwrap();
    assert_eq!(v, Value::String("TypeError".to_string()));
}

#[test]
fn strict_function_arguments_get_throws_typeerror() {
    let mut ctx = Context::new().unwrap();
    let v = ctx
        .eval(
            "function f() { 'use strict'; }
             try { var x = f.arguments; 'no throw'; } catch(e) { e.name; }",
        )
        .unwrap();
    assert_eq!(v, Value::String("TypeError".to_string()));
}

#[test]
fn bound_function_caller_get_throws_typeerror() {
    let mut ctx = Context::new().unwrap();
    let v = ctx
        .eval(
            "function target() {}
             var bound = target.bind({});
             try { var x = bound.caller; 'no throw'; } catch(e) { e.name; }",
        )
        .unwrap();
    assert_eq!(v, Value::String("TypeError".to_string()));
}

#[test]
fn bound_function_arguments_get_throws_typeerror() {
    let mut ctx = Context::new().unwrap();
    let v = ctx
        .eval(
            "function target() {}
             var bound = target.bind({});
             try { var x = bound.arguments; 'no throw'; } catch(e) { e.name; }",
        )
        .unwrap();
    assert_eq!(v, Value::String("TypeError".to_string()));
}

#[test]
fn var_arguments_does_not_shadow_builtin() {
    let mut ctx = Context::new().unwrap();
    let v = ctx
        .eval("function f() { var arguments = 42; return typeof arguments; } f()")
        .unwrap();
    assert_eq!(v, Value::String("number".to_string()));
}

#[test]
fn typeof_arguments_in_function_with_var() {
    let mut ctx = Context::new().unwrap();
    let v = ctx
        .eval("function f() { return typeof arguments; var arguments = 1; } f()")
        .unwrap();
    assert_eq!(v, Value::String("object".to_string()));
}

#[test]
fn inner_function_decl_hoisted_in_function_body() {
    let mut ctx = Context::new().unwrap();
    let v = ctx
        .eval(
            "function f() {
               function g() { return 'inner'; }
               return g();
             }
             f()",
        )
        .unwrap();
    assert_eq!(v, Value::String("inner".to_string()));
}

#[test]
fn inner_function_decl_hoisted_called_before_declaration() {
    let mut ctx = Context::new().unwrap();
    let v = ctx
        .eval(
            "function f() {
               return g();
               function g() { return 'hoisted'; }
             }
             f()",
        )
        .unwrap();
    assert_eq!(v, Value::String("hoisted".to_string()));
}

#[test]
fn function_prototype_isprototype_of_own_function() {
    let mut ctx = Context::new().unwrap();
    let v = ctx
        .eval("function f(){} Function.prototype.isPrototypeOf(f)")
        .unwrap();
    assert_eq!(v, Value::Boolean(true));
}

#[test]
fn function_expr_prototype_isprototype_of() {
    let mut ctx = Context::new().unwrap();
    let v = ctx
        .eval("var f = function(){}; Function.prototype.isPrototypeOf(f)")
        .unwrap();
    assert_eq!(v, Value::Boolean(true));
}

#[test]
fn constructor_prototype_isprototype_of_instance() {
    let mut ctx = Context::new().unwrap();
    let v = ctx
        .eval("function F(){}; var p = {}; F.prototype = p; var o = new F(); p.isPrototypeOf(o)")
        .unwrap();
    assert_eq!(v, Value::Boolean(true));
}

#[test]
fn function_prototype_isprototype_of_constructed_instance() {
    let mut ctx = Context::new().unwrap();
    let value = ctx
        .eval("function P(){} function F(){} F.prototype=P; F.prototype === P")
        .unwrap();
    assert_eq!(value, Value::Boolean(true));
    let value = ctx
        .eval("var value=new F(); P.isPrototypeOf(value)")
        .unwrap();
    assert_eq!(value, Value::Boolean(true));
}

#[test]
fn function_used_as_prototype_exposes_its_properties() {
    let mut ctx = Context::new().unwrap();
    let value = ctx
        .eval("function P(){} P.type='x'; function F(){} F.prototype=P; new F().type")
        .unwrap();
    assert_eq!(value, Value::String("x".to_string()));
}

#[test]
fn var_function_initializer_inside_with_stays_callable() {
    let mut ctx = Context::new().unwrap();
    let value = ctx
        .eval("var a=1; var o={a:2}; with(o) { var f=function(){return a;} } typeof f")
        .unwrap();
    assert_eq!(value, Value::String("function".to_string()));
}

#[test]
fn var_function_initializer_inside_with_can_be_called_after_scope() {
    let mut ctx = Context::new().unwrap();
    let value = ctx
        .eval("var a=1; var o={a:2}; with(o) { var f=function(){return a;} } f()")
        .unwrap();
    assert_eq!(value, Value::Number(2.0));
}

#[test]

#[test]
fn global_this_inherits_object_prototype_for_with_function_case() {
    use crate::test262::host::{QuenchHost, Test262Host};

    let mut host = QuenchHost::new();
    let result = host.run_script(
        "var a=1; var __obj={a:2}; with(__obj){ var __func=function(){return a;} } this.hasOwnProperty('__func')",
    );
    assert!(result.is_ok(), "globalThis prototype lookup failed: {result:?}");
}

#[test]
fn failed_constructor_restores_new_target_for_following_calls() {
    let mut ctx = Context::new().unwrap();
    let result = ctx.eval(
        "function F(){ missing(); } try { new F(); } catch (e) {} Symbol('after failure')",
    );
    assert!(matches!(result, Ok(Value::Symbol(_))), "unexpected result: {result:?}");
}

#[test]
fn plain_object_call_does_not_use_constructor_property() {
    let mut ctx = Context::new().unwrap();
    let result = ctx.eval("var value={constructor:function(){}}; value()");
    assert!(result.is_err());
    assert!(result.unwrap_err().0.contains("not a function"));
}

#[test]
fn deleting_mapped_arguments_index_removes_property() {
    let mut ctx = Context::new().unwrap();
    let v = ctx
        .eval(
            "function f(value) { delete arguments[0]; return typeof arguments[0]; } f(1)",
        )
        .unwrap();
    assert_eq!(v, Value::String("undefined".to_string()));
}

#[test]
fn deleting_mapped_arguments_index_in_harness_host_removes_property() {
    use crate::test262::host::{QuenchHost, Test262Host};

    let mut host = QuenchHost::new();
    let result = host.run_script(
        "function f(value) { delete arguments[0]; if (arguments[0] !== undefined) throw new Error('not deleted'); } f(1);",
    );
    assert!(result.is_ok(), "arguments deletion failed: {result:?}");
}

#[test]
fn deleting_strict_arguments_index_removes_property() {
    let mut ctx = Context::new().unwrap();
    let v = ctx
        .eval("function f(value) { 'use strict'; delete arguments[0]; return typeof arguments[0]; } f(1)")
        .unwrap();
    assert_eq!(v, Value::String("undefined".to_string()));
}

#[test]
fn duplicate_function_declarations_use_last_hoisted_binding() {
    let mut ctx = Context::new().unwrap();
    let v = ctx
        .eval(
            "function f() { function g() { return 'first'; } var saved = g; function g() { return 'second'; } return saved() + ':' + g(); } f()",
        )
        .unwrap();
    assert_eq!(v, Value::String("second:second".to_string()));
}

#[test]
fn var_initializer_overwrites_hoisted_function_binding() {
    let mut ctx = Context::new().unwrap();
    let v = ctx
        .eval("(function () { if (typeof value !== 'function') return 'bad'; var value = 1; return value; function value() {} })()")
        .unwrap();
    assert_eq!(v, Value::Number(1.0));
}

#[test]
fn function_prototype_constructor_is_not_enumerable() {
    let mut ctx = Context::new().unwrap();
    let v = ctx
        .eval("function f() {} var keys = []; for (var key in f.prototype) keys.push(key); keys.join(',')")
        .unwrap();
    assert_eq!(v, Value::String("".to_string()));
}
