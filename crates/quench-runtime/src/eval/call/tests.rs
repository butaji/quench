//! Unit tests for call evaluation — removed all test262-replica tests.

use crate::builtins;
use crate::value::Value;
use crate::Context;

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
