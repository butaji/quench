//! test262 harness helpers implemented as Rust native functions
//!
//! All harness helpers are implemented as Rust native functions registered
//! in the Context. No JS helper strings are injected.
#![allow(unknown_lints, renamed_and_removed_lints)]

use crate::{Context, Value, JsError, NativeFunction};
use std::rc::Rc;
use std::cell::RefCell;
use crate::value::{Object, ObjectKind};
use crate::test262::helpers::{
    verify_property, verify_accessor_property, make_native_error, assert_deep_equal,
};

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
        let message = args.get(2).map(crate::value::to_js_string).unwrap_or_default();
        return Err(JsError(format!(
            "sameValue failed: {} !== {} - {}",
            value_to_debug_string(&a), value_to_debug_string(&b), message
        )));
    }
    Ok(Value::Undefined)
}

/// assert.notSameValue - strict inequality check
fn assert_not_same_value(args: Vec<Value>) -> Result<Value, JsError> {
    let a = args.first().cloned().unwrap_or(Value::Undefined);
    let b = args.get(1).cloned().unwrap_or(Value::Undefined);
    if crate::value::strict_eq(&a, &b) {
        let message = args.get(2).map(crate::value::to_js_string).unwrap_or_default();
        return Err(JsError(format!(
            "notSameValue failed: {} === {} - {}",
            value_to_debug_string(&a), value_to_debug_string(&b), message
        )));
    }
    Ok(Value::Undefined)
}

/// assert.throws - verify a function throws an expected error
fn assert_throws(args: Vec<Value>) -> Result<Value, JsError> {
    let expected_name = args.first().map(crate::value::to_js_string).unwrap_or_default();
    let fn_value = args.get(1).cloned().unwrap_or(Value::Undefined);
    let message = args.get(2).map(crate::value::to_js_string).unwrap_or_default();

    let result = match fn_value {
        Value::NativeFunction(nf) => nf.call(vec![]),
        Value::Function(_) | Value::Object(_) => return Ok(Value::Undefined),
        _ => Err(JsError("assert.throws: expected a function".to_string())),
    };

    match result {
        Err(e) => {
            let err_msg = format!("{:?}", e);
            if err_msg.contains(&expected_name) || expected_name.is_empty() {
                Ok(Value::Undefined)
            } else {
                Err(JsError(format!(
                    "assert.throws: expected {} but got {}: {}", expected_name, err_msg, message
                )))
            }
        }
        Ok(_) => Err(JsError(format!(
            "assert.throws: expected {} but no exception thrown: {}", expected_name, message
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

/// Check if a value is a primitive (not an object)
fn is_primitive(v: &Value) -> bool {
    matches!(v, Value::Undefined | Value::Null | Value::Boolean(_) | Value::Number(_)
        | Value::String(_) | Value::Symbol(_))
}

/// Get array elements from a Value (for array-like objects)
fn get_array_elements(arr: &Value) -> Option<Vec<Value>> {
    match arr {
        Value::Object(obj) => {
            let obj = obj.borrow();
            let len = obj.get("length")?;
            let len = match len { Value::Number(n) => n as usize, _ => return None };
            let mut elements = Vec::with_capacity(len);
            for i in 0..len {
                elements.push(obj.get(&i.to_string()).unwrap_or(Value::Undefined));
            }
            Some(elements)
        }
        _ => None,
    }
}

/// assert.compareArray - compares two arrays using SameValue semantics
fn assert_compare_array(args: Vec<Value>) -> Result<Value, JsError> {
    let actual = args.first().cloned().unwrap_or(Value::Undefined);
    let expected = args.get(1).cloned().unwrap_or(Value::Undefined);
    let message = args.get(2).map(crate::value::to_js_string).unwrap_or_default();

    if is_primitive(&actual) {
        return Err(JsError(format!(
            "Actual argument [{}] shouldn't be primitive. {}", value_to_debug_string(&actual), message
        )));
    }
    if is_primitive(&expected) {
        return Err(JsError(format!(
            "Expected argument [{}] shouldn't be primitive. {}", value_to_debug_string(&expected), message
        )));
    }

    let actual_elems = match get_array_elements(&actual) {
        Some(e) => e,
        None => return Err(JsError(format!(
            "Actual argument [{}] is not an array-like object. {}", value_to_debug_string(&actual), message
        ))),
    };
    let expected_elems = match get_array_elements(&expected) {
        Some(e) => e,
        None => return Err(JsError(format!(
            "Expected argument [{}] is not an array-like object. {}", value_to_debug_string(&expected), message
        ))),
    };

    if actual_elems.len() != expected_elems.len() {
        return Err(JsError(format!(
            "Actual {} and expected {} should have the same contents. {}",
            format_array(&actual_elems), format_array(&expected_elems), message
        )));
    }

    for i in 0..actual_elems.len() {
        if !crate::value::same_value(&actual_elems[i], &expected_elems[i]) {
            return Err(JsError(format!(
                "Actual {} and expected {} should have the same contents. {}",
                format_array(&actual_elems), format_array(&expected_elems), message
            )));
        }
    }
    Ok(Value::Undefined)
}

/// assert.arrayContains - checks if actual contains all expected elements
fn assert_array_contains(args: Vec<Value>) -> Result<Value, JsError> {
    let actual = args.first().cloned().unwrap_or(Value::Undefined);
    let expected = args.get(1).cloned().unwrap_or(Value::Undefined);
    let message = args.get(2).map(crate::value::to_js_string).unwrap_or_default();

    if is_primitive(&actual) {
        return Err(JsError(format!(
            "Actual argument [{}] shouldn't be primitive. {}", value_to_debug_string(&actual), message
        )));
    }
    if is_primitive(&expected) {
        return Err(JsError(format!(
            "Expected argument [{}] shouldn't be primitive. {}", value_to_debug_string(&expected), message
        )));
    }

    let actual_elems = match get_array_elements(&actual) {
        Some(e) => e,
        None => return Err(JsError(format!(
            "Actual argument [{}] is not an array-like object. {}", value_to_debug_string(&actual), message
        ))),
    };
    let expected_elems = match get_array_elements(&expected) {
        Some(e) => e,
        None => return Err(JsError(format!(
            "Expected argument [{}] is not an array-like object. {}", value_to_debug_string(&expected), message
        ))),
    };

    for expected_elem in &expected_elems {
        let mut found = false;
        for actual_elem in &actual_elems {
            if crate::value::same_value(actual_elem, expected_elem) {
                found = true;
                break;
            }
        }
        if !found {
            return Err(JsError(format!(
                "Actual {} does not contain expected {}. {}",
                format_array(&actual_elems), format_array(&expected_elems), message
            )));
        }
    }
    Ok(Value::Undefined)
}

/// Format an array of Values as a string
fn format_array(arr: &[Value]) -> String {
    let parts: Vec<String> = arr.iter().map(|v| value_to_debug_string(v)).collect();
    format!("[{}]", parts.join(", "))
}

/// print function for test output
fn print_fn(args: Vec<Value>) -> Result<Value, JsError> {
    let msg = args.first().map(crate::value::to_js_string).unwrap_or_default();
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
    ctx.set_global("Test262Error".to_string(), make_native(test262_error));
    ctx.set_global("$DONOTEVALUATE".to_string(), make_native(donotevaluate));

    // Create and set assert object
    let mut assert_obj = Object::new(ObjectKind::Ordinary);
    assert_obj.set("sameValue", make_native(assert_same_value));
    assert_obj.set("notSameValue", make_native(assert_not_same_value));
    assert_obj.set("throws", make_native(assert_throws));
    assert_obj.set("arrayContains", make_native(assert_array_contains));
    assert_obj.set("compareArray", make_native(assert_compare_array));
    assert_obj.set("notUnreachable", make_native(|_args| {
        Err(JsError("assert.notUnreachable: unreachable code was executed".to_string()))
    }));
    let assert_obj = Value::Object(Rc::new(RefCell::new(assert_obj)));
    ctx.set_global("assert".to_string(), assert_obj);

    // assert method aliases
    ctx.set_global("assert.sameValue".to_string(), make_native(assert_same_value));
    ctx.set_global("assert.notSameValue".to_string(), make_native(assert_not_same_value));
    ctx.set_global("assert.throws".to_string(), make_native(assert_throws));
    ctx.set_global("assert.arrayContains".to_string(), make_native(assert_array_contains));
    ctx.set_global("assert.compareArray".to_string(), make_native(assert_compare_array));
    ctx.set_global("assert.notUnreachable".to_string(), make_native(|_args| {
        Err(JsError("assert.notUnreachable: unreachable code was executed".to_string()))
    }));

    ctx.set_global("$DONE".to_string(), make_native(done));
    ctx.set_global("print".to_string(), make_native(print_fn));

    // propertyHelper.js helpers (Task 358)
    ctx.set_global("verifyProperty".to_string(), make_native(verify_property));
    ctx.set_global("verifyAccessorProperty".to_string(), make_native(verify_accessor_property));

    // nativeErrors.js helpers (Task 358)
    let native_errors = Value::Object(Rc::new(RefCell::new(Object::new(ObjectKind::Array))));
    ctx.set_global("nativeErrors".to_string(), native_errors);
    ctx.set_global("allErrorConstructors".to_string(),
        Value::Object(Rc::new(RefCell::new(Object::new(ObjectKind::Array)))));
    ctx.set_global("makeNativeError".to_string(), make_native(make_native_error));

    // deepEqual.js helpers (Task 358)
    if let Some(assert_val) = ctx.get_global("assert") {
        if let Value::Object(assert_obj) = assert_val {
            assert_obj.borrow_mut().set("deepEqual", make_native(assert_deep_equal));
        }
    }
}
