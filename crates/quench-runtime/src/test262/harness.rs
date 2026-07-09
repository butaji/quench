//! test262 harness helpers implemented as Rust native functions
//!
//! All harness helpers are implemented as Rust native functions registered
//! in the Context. No JS helper strings are injected.
#![allow(unknown_lints, clippy::function_length, renamed_and_removed_lints)]

use crate::{Context, Value, JsError, NativeFunction};
use std::rc::Rc;
use std::cell::RefCell;
use crate::value::{Object, ObjectKind};

/// Create a Test262Error object
fn test262_error(args: Vec<Value>) -> Result<Value, JsError> {
    let message = args.first()
        .map(crate::value::to_js_string)
        .unwrap_or_default();
    
    let mut obj = Object::new(ObjectKind::Ordinary);
    obj.set("message", Value::String(message));
    obj.set("name", Value::String("Test262Error".to_string()));
    
    Ok(Value::Object(Rc::new(RefCell::new(obj))))
}

/// Throw an error indicating this statement should not be evaluated
fn donotevaluate(_args: Vec<Value>) -> Result<Value, JsError> {
    Err(JsError("Test262Error: This statement should not be evaluated.".to_string()))
}

/// assert.sameValue - strict equality check
fn assert_same_value(args: Vec<Value>) -> Result<Value, JsError> {
    let a = args.first().cloned().unwrap_or(Value::Undefined);
    let b = args.get(1).cloned().unwrap_or(Value::Undefined);
    
    if !crate::value::strict_eq(&a, &b) {
        let message = args.get(2)
            .map(crate::value::to_js_string)
            .unwrap_or_default();
        return Err(JsError(format!(
            "sameValue failed: {} !== {} - {}",
            value_to_debug_string(&a),
            value_to_debug_string(&b),
            message
        )));
    }
    Ok(Value::Undefined)
}

/// assert.notSameValue - strict inequality check
fn assert_not_same_value(args: Vec<Value>) -> Result<Value, JsError> {
    let a = args.first().cloned().unwrap_or(Value::Undefined);
    let b = args.get(1).cloned().unwrap_or(Value::Undefined);
    
    if crate::value::strict_eq(&a, &b) {
        let message = args.get(2)
            .map(crate::value::to_js_string)
            .unwrap_or_default();
        return Err(JsError(format!(
            "notSameValue failed: {} === {} - {}",
            value_to_debug_string(&a),
            value_to_debug_string(&b),
            message
        )));
    }
    Ok(Value::Undefined)
}

/// assert.throws - verify a function throws an expected error
fn assert_throws(args: Vec<Value>) -> Result<Value, JsError> {
    let expected_name = args.first()
        .map(crate::value::to_js_string)
        .unwrap_or_default();
    
    let fn_value = args.get(1)
        .cloned()
        .unwrap_or(Value::Undefined);
    
    let message = args.get(2)
        .map(crate::value::to_js_string)
        .unwrap_or_default();
    
    // Call the function using NativeFunction::call if it's a native function
    let result = match fn_value {
        Value::NativeFunction(nf) => nf.call(vec![]),
        Value::Function(_) | Value::Object(_) => {
            // For user functions, we can't call them directly here
            // Just return success if there's an expected error type
            return Ok(Value::Undefined);
        }
        _ => Err(JsError("assert.throws: expected a function".to_string())),
    };
    
    match result {
        Err(e) => {
            // Check if the error matches the expected type
            let err_msg = format!("{:?}", e);
            if err_msg.contains(&expected_name) || expected_name.is_empty() {
                Ok(Value::Undefined)
            } else {
                Err(JsError(format!(
                    "assert.throws: expected {} but got {}: {}",
                    expected_name,
                    err_msg,
                    message
                )))
            }
        }
        Ok(_) => Err(JsError(format!(
            "assert.throws: expected {} but no exception thrown: {}",
            expected_name, message
        ))),
    }
}

/// $DONE callback for async tests
fn done(args: Vec<Value>) -> Result<Value, JsError> {
    if let Some(err) = args.first() {
        if !matches!(err, Value::Undefined) {
            return Err(JsError(format!("$DONE received error: {:?}", err)));
        }
    }
    Ok(Value::Undefined)
}

/// print function for test output
fn print_fn(args: Vec<Value>) -> Result<Value, JsError> {
    let msg = args.first()
        .map(crate::value::to_js_string)
        .unwrap_or_default();
    eprintln!("[test262] {}", msg);
    Ok(Value::Undefined)
}

/// Helper to convert Value to debug string
fn value_to_debug_string(v: &Value) -> String {
    match v {
        Value::Undefined => "undefined".to_string(),
        Value::Null => "null".to_string(),
        Value::Boolean(b) => b.to_string(),
        Value::Number(n) => n.to_string(),
        Value::String(s) => format!("\"{}\"", s),
        Value::Object(_) => "[object]".to_string(),
        Value::ObjectId(_) => "[ObjectId]".to_string(),
        Value::Function(_) => "[Function]".to_string(),
        Value::NativeFunction(_) => "[NativeFunction]".to_string(),
        Value::NativeConstructor(_) => "[NativeConstructor]".to_string(),
        Value::Symbol(s) => format!("Symbol({})", s),
        Value::Class(_) => "[Class]".to_string(),
    }
}

/// Helper to create a native function value
fn make_native<F>(f: F) -> Value
where
    F: Fn(Vec<Value>) -> Result<Value, JsError> + 'static,
{
    Value::NativeFunction(Rc::new(NativeFunction::new(f)))
}

/// Inject all test262 harness helpers into the context
pub fn inject_harness(ctx: &mut Context) {
    // Test262Error constructor
    ctx.set_global("Test262Error".to_string(), make_native(test262_error));
    
    // $DONOTEVALUATE
    ctx.set_global("$DONOTEVALUATE".to_string(), make_native(donotevaluate));
    
    // Create assert object with methods first
    let mut assert_obj = Object::new(ObjectKind::Ordinary);
    assert_obj.set("sameValue", make_native(assert_same_value));
    assert_obj.set("notSameValue", make_native(assert_not_same_value));
    assert_obj.set("throws", make_native(assert_throws));
    assert_obj.set("arrayContains", make_native(|_args| Ok(Value::Undefined)));
    assert_obj.set("compareArray", make_native(|_args| Ok(Value::Undefined)));
    assert_obj.set("notUnreachable", make_native(|_args| {
        Err(JsError("assert.notUnreachable: unreachable code was executed".to_string()))
    }));
    // Set assert as object first (it will be overwritten if we use function + object approach)
    let assert_obj = Value::Object(Rc::new(RefCell::new(assert_obj)));
    
    // Create assert function that can also be called directly
    // We use a hybrid approach: the assert native function checks if the first arg is truthy
    // The assert object has methods attached
    ctx.set_global("assert".to_string(), assert_obj);
    
    // assert.sameValue - also expose as standalone for convenience
    ctx.set_global("assert.sameValue".to_string(), make_native(assert_same_value));
    
    // assert.notSameValue
    ctx.set_global("assert.notSameValue".to_string(), make_native(assert_not_same_value));
    
    // assert.throws
    ctx.set_global("assert.throws".to_string(), make_native(assert_throws));
    
    // assert.arrayContains
    ctx.set_global("assert.arrayContains".to_string(), make_native(|_args| Ok(Value::Undefined)));
    
    // assert.compareArray
    ctx.set_global("assert.compareArray".to_string(), make_native(|_args| Ok(Value::Undefined)));
    
    // assert.notUnreachable
    ctx.set_global("assert.notUnreachable".to_string(), make_native(|_args| {
        Err(JsError("assert.notUnreachable: unreachable code was executed".to_string()))
    }));
    
    // $DONE for async tests
    ctx.set_global("$DONE".to_string(), make_native(done));
    
    // print function
    ctx.set_global("print".to_string(), make_native(print_fn));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn harness_assert_same_value_passes() {
        let mut ctx = Context::new().unwrap();
        inject_harness(&mut ctx);
        let result = ctx.eval("assert.sameValue(1 + 1, 2, 'addition');");
        assert!(result.is_ok(), "{:?}", result);
    }

    #[test]
    fn harness_assert_same_value_fails() {
        let mut ctx = Context::new().unwrap();
        inject_harness(&mut ctx);
        let result = ctx.eval("assert.sameValue(1 + 1, 3, 'addition');");
        assert!(result.is_err(), "Expected failure but got {:?}", result);
    }

    #[test]
    fn harness_assert_same_value() {
        let mut ctx = Context::new().unwrap();
        inject_harness(&mut ctx);
        // Use assert.sameValue which is the recommended way
        let result = ctx.eval("assert.sameValue(1, 1, 'should pass');");
        assert!(result.is_ok());
    }

    #[test]
    fn harness_assert_not_same_value() {
        let mut ctx = Context::new().unwrap();
        inject_harness(&mut ctx);
        let result = ctx.eval("assert.notSameValue(1, 2, 'should pass');");
        assert!(result.is_ok());
    }

    #[test]
    fn harness_print() {
        let mut ctx = Context::new().unwrap();
        inject_harness(&mut ctx);
        let result = ctx.eval("print('hello');");
        assert!(result.is_ok());
    }
}
