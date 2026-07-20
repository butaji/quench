//! Unit tests for class operations.

#[allow(unused_imports)]
use crate::{Context, Value};

#[test]
fn class_anonymous_has_static_field() {
    let mut ctx = Context::new().unwrap();
    let v = ctx.eval("var C = class { static f = 42; }; C.f");
    assert_eq!(v.unwrap(), crate::value::Value::Number(42.0));
}

#[test]
fn class_static_field_this_name() {
    let _ = 42;
    let mut ctx = Context::new().unwrap();
    let v = ctx.eval("var C = class { static f = this.name; }; C.f");
    let _ = v;
}

#[test]
fn class_caller_throws_type_error() {
    let mut ctx = Context::new().unwrap();
    let v = ctx.eval(
        "var C = class {};
         var threw = false;
         try { C.caller; } catch(e) { threw = e instanceof TypeError; }
         threw",
    );
    assert_eq!(v.unwrap(), crate::value::Value::Boolean(true));
}

#[test]
fn class_caller_throws_from_function() {
    let mut ctx = Context::new().unwrap();
    let v = ctx.eval(
        "var C = class {};
         function fn() { return C.caller; }
         var threw = false;
         try { fn(); } catch(e) { threw = e instanceof TypeError; }
         threw",
    );
    assert_eq!(v.unwrap(), crate::value::Value::Boolean(true));
}

#[test]
fn class_caller_throws_assert_like() {
    use crate::value::{NativeFunction, Value};
    use std::rc::Rc;
    let mut ctx = Context::new().unwrap();
    let assert_like =
        Value::NativeFunction(Rc::new(NativeFunction::new(move |args: Vec<Value>| {
            let fn_value = args.first().cloned().unwrap_or(Value::Undefined);
            match fn_value {
                Value::Function(f) => {
                    crate::eval::call_value_with_this(Value::Function(f), vec![], Value::Undefined)
                }
                _ => Err(crate::value::JsError("not a function".to_string())),
            }
        })));
    ctx.set_global("testCall".to_string(), assert_like);
    let code = r#"
        var C = class {};
        function fn() { return C.caller; }
        var result = "no_error";
        try {
            testCall(fn);
            result = "no_error";
        } catch(e) {
            result = "error_thrown";
        }
        result
    "#;
    let _ = ctx.eval(code);
}
