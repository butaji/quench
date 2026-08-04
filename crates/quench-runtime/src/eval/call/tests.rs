//! Unit tests for call evaluation — removed all test262-replica tests.

use crate::builtins;
use crate::value::Value;
use crate::Context;

#[test]
fn call_does_not_evaluate_arguments_for_non_callable_member() {
    let mut ctx = Context::new().unwrap();
    assert_eq!(
        ctx.eval("var called=false; function foo(){called=true} var o={}; try { o.bar.gar(foo()) } catch(e) {} called"),
        Ok(Value::Boolean(false))
    );
}

#[test]
fn new_evaluates_arguments_before_constructor_validation() {
    let mut ctx = Context::new().unwrap();
    let value = ctx.eval("var x = {}; try { new x(x = Array); } catch (e) {} x === Array");
    assert_eq!(value, Ok(Value::Boolean(true)));
}

#[test]
fn implicit_global_assignment_creates_enumerable_property() {
    let mut ctx = Context::new().unwrap();
    let value = ctx.eval("function f() { __implicit_global_probe__ = 42; } f(); Object.getOwnPropertyDescriptor(this, '__implicit_global_probe__').enumerable");
    assert_eq!(value, Ok(Value::Boolean(true)));
}

#[test]
fn object_methods_are_not_constructable() {
    let mut ctx = Context::new().unwrap();
    let value = ctx.eval("var obj = { method() {} }; try { new obj.method(); false; } catch (e) { e instanceof TypeError; }");
    assert_eq!(value, Ok(Value::Boolean(true)));
}

#[test]
fn call_through_with_environment_uses_with_object_as_this() {
    let mut ctx = Context::new().unwrap();
    assert_eq!(
        ctx.eval("var obj={method:function(){return this}}; with(obj){objResult=method()} objResult===obj"),
        Ok(Value::Boolean(true))
    );
}

#[test]
fn arguments_index_descriptor_is_enumerable() {
    let mut ctx = Context::new().unwrap();
    let result = ctx
        .eval("var data = 'data'; Object.defineProperty(Object.prototype, '0', {get: function() { return data; }, set: function() { data = 'changed'; }, configurable: true}); var a = (function() { return arguments; })(1); var d = Object.getOwnPropertyDescriptor(a, '0'); var keys = []; for (var k in a) keys.push(k); [d.value, d.writable, d.enumerable, d.configurable, data, keys.join(',')].join('|')")
        .unwrap();
    assert_eq!(result, Value::String("1|true|true|true|data|0".into()));
}

#[test]
fn sloppy_arguments_callee_is_writable() {
    let mut ctx = Context::new().unwrap();
    let result = ctx
        .eval("Object.defineProperty(Object.prototype, 'callee', {value: 1, writable: false, configurable: true}); var a = (function() { return arguments; })(1); var d = Object.getOwnPropertyDescriptor(a, 'callee'); a.callee = 2; [d.writable, d.enumerable, d.configurable, a.callee].join('|')")
        .unwrap();
    assert_eq!(result, Value::String("true|false|true|2".into()));
}

#[test]
fn arguments_length_is_writable_own_data_property() {
    let mut ctx = Context::new().unwrap();
    let result = ctx
        .eval("Object.defineProperty(Object.prototype, 'length', {get: function() { return 12; }, set: function() {}, configurable: true}); var a = (function() { return arguments; })(1); var d = Object.getOwnPropertyDescriptor(a, 'length'); a.length = 2; [d.writable, d.enumerable, d.configurable, a.length].join('|')")
        .unwrap();
    assert_eq!(result, Value::String("true|false|true|2".into()));
}

/// ES spec: arguments object with spread call must populate indexed properties.
/// Regression: mappable arguments (sloppy mode, no params, no rest/default)
/// stored values only in `elements` but Array.prototype.map's get_this_array
/// for non-Array objects only checked `properties`, missing the elements.
#[test]
fn test_spread_call_arguments_sloppy() {
    let mut ctx = Context::new().unwrap();
    builtins::register_builtins(&mut ctx);
    let r = ctx.eval(
        r#"
        function f() { return arguments; }
        var args = f(...[0, 'a', undefined]);
        args[0] === 0 && args[1] === 'a' && args[2] === undefined;
        "#,
    );
    assert_eq!(r, Ok(Value::Boolean(true)));
}

/// Regression: arguments object with no params in sloppy mode stores values
/// in `elements` but `Object::get_own` for `ObjData::Args` didn't read them.
/// This means `arguments[n]` used by `Object::get` (e.g. via Rust native
/// helpers like `get_array_elements`) returned `undefined`.
#[test]
fn test_spread_call_arguments_sloppy_no_params_object_get() {
    let mut ctx = Context::new().unwrap();
    builtins::register_builtins(&mut ctx);
    let args_obj = ctx
        .eval("function f() { return arguments; } f(...[0, 'a', undefined]);")
        .unwrap();
    let Value::Object(ref obj_rc) = args_obj else {
        panic!("expected object");
    };
    let obj = obj_rc.borrow();
    assert_eq!(obj.get("0"), Some(Value::Number(0.0)));
    assert_eq!(obj.get("1"), Some(Value::String("a".into())));
    assert_eq!(obj.get("2"), Some(Value::Undefined));
}

/// Same test but verifying Array.prototype.map works on arguments
/// (the format function in compareArray uses this).
#[test]
fn test_spread_call_arguments_map() {
    let mut ctx = Context::new().unwrap();
    builtins::register_builtins(&mut ctx);
    // Use `JSON.stringify` to serialize the mapped result as a string
    let r = ctx.eval(
        r#"
        function f() {
            var mapped = Array.prototype.map.call(arguments, function(v) {
                return v === undefined ? 'undefined' : String(v);
            });
            return mapped.join(",");
        }
        f(...[0, 'a', undefined]);
        "#,
    );
    assert_eq!(r, Ok(Value::String("0,a,undefined".into())));
}
