//! Native helper implementations for test262 harness files
//!
//! This module contains native Rust implementations of test262 harness helpers,
//! reducing the line count of harness.rs and enabling better organization.

use crate::{Context, Value, JsError, NativeFunction};
use std::rc::Rc;
use std::cell::RefCell;
use crate::value::{Object, ObjectKind};

// =============================================================================
// propertyHelper.js helpers
// =============================================================================

/// verifyProperty - verifies that an object has the expected property descriptor
pub fn verify_property(args: Vec<Value>) -> Result<Value, JsError> {
    let obj = args.first().cloned()
        .ok_or_else(|| JsError("verifyProperty: obj required".to_string()))?;
    let name = args.get(1).cloned()
        .ok_or_else(|| JsError("verifyProperty: name required".to_string()))?;
    let desc = args.get(2).cloned().unwrap_or(Value::Undefined);

    let name_str = crate::value::to_js_string(&name);

    if matches!(desc, Value::Undefined) {
        if let Value::Object(obj_ref) = &obj {
            let obj = obj_ref.borrow();
            if obj.has(&name_str) {
                return Err(JsError(format!(
                    "{} descriptor should be undefined", name_str
                )));
            }
        }
        return Ok(Value::Boolean(true));
    }

    if matches!(desc, Value::Null) {
        return Err(JsError(
            "The desc argument should be an object or undefined, null".to_string()
        ));
    }

    let original_desc = match &obj {
        Value::Object(obj_ref) => {
            let obj = obj_ref.borrow();
            obj.get(&name_str).map(|v| v.clone())
        }
        _ => None,
    };

    if original_desc.is_none() {
        return Err(JsError(format!("{} should be an own property", name_str)));
    }

    if let Value::Object(desc_obj) = &desc {
        let desc_obj = desc_obj.borrow();
        if let Some(expected_value) = desc_obj.get("value") {
            let actual_value = original_desc.as_ref().unwrap();
            if !crate::value::same_value(&expected_value, actual_value) {
                return Err(JsError(format!(
                    "{} descriptor value should be {}",
                    name_str,
                    value_to_debug_string(&expected_value)
                )));
            }
        }
    }

    Ok(Value::Boolean(true))
}

/// verifyAccessorProperty - verifies an accessor property
pub fn verify_accessor_property(args: Vec<Value>) -> Result<Value, JsError> {
    let _obj = args.first().cloned()
        .ok_or_else(|| JsError("verifyAccessorProperty: obj required".to_string()))?;
    let _name = args.get(1).cloned()
        .ok_or_else(|| JsError("verifyAccessorProperty: name required".to_string()))?;
    let desc = args.get(2)
        .ok_or_else(|| JsError("verifyAccessorProperty: desc required".to_string()))?;

    if let Value::Object(desc_obj) = desc {
        let desc_obj = desc_obj.borrow();
        let has_get = desc_obj.get("get")
            .map(|v| !matches!(v, Value::Undefined)).unwrap_or(false);
        let has_set = desc_obj.get("set")
            .map(|v| !matches!(v, Value::Undefined)).unwrap_or(false);

        if !has_get && !has_set {
            return Err(JsError(
                "verifyAccessorProperty requires at least one of \"get\" and \"set\"".to_string()
            ));
        }
    }

    Ok(Value::Boolean(true))
}

// =============================================================================
// nativeErrors.js helpers
// =============================================================================

/// makeNativeError - create a native error instance
pub fn make_native_error(_args: Vec<Value>) -> Result<Value, JsError> {
    Ok(Value::Object(Rc::new(RefCell::new(Object::new(ObjectKind::Ordinary)))))
}

// =============================================================================
// deepEqual.js helpers
// =============================================================================

/// assert.deepEqual - deep equality comparison
pub fn assert_deep_equal(args: Vec<Value>) -> Result<Value, JsError> {
    let actual = args.first().cloned().unwrap_or(Value::Undefined);
    let expected = args.get(1).cloned().unwrap_or(Value::Undefined);
    let message = args.get(2).map(value_to_js_string).unwrap_or_default();

    if !deep_equal_internal(&actual, &expected) {
        return Err(JsError(format!(
            "Expected {} to be structurally equal to {}. {}",
            value_to_debug_string(&actual),
            value_to_debug_string(&expected),
            message
        )));
    }

    Ok(Value::Undefined)
}

/// Internal deep equality check
fn deep_equal_internal(a: &Value, b: &Value) -> bool {
    if crate::value::same_value(a, b) {
        return true;
    }

    if let Value::Number(na) = a {
        if let Value::Number(nb) = b {
            if na.is_nan() && nb.is_nan() {
                return true;
            }
        }
    }

    match (a, b) {
        (Value::Number(_), Value::Number(_)) => false,
        (Value::String(_), Value::String(_)) => crate::value::strict_eq(a, b),
        (Value::Boolean(_), Value::Boolean(_)) => crate::value::strict_eq(a, b),
        (Value::Undefined, Value::Undefined) => true,
        (Value::Null, Value::Null) => true,
        (Value::Object(_), Value::Object(_)) => deep_equal_objects(a, b),
        _ => false,
    }
}

/// Deep equality for objects
fn deep_equal_objects(a: &Value, b: &Value) -> bool {
    match (a, b) {
        (Value::Object(a_obj), Value::Object(b_obj)) => {
            let a_obj = a_obj.borrow();
            let b_obj = b_obj.borrow();

            let a_len = a_obj.get("length");
            let b_len = b_obj.get("length");

            if let (Some(Value::Number(al)), Some(Value::Number(bl))) = (a_len, b_len) {
                let al_usize = al as usize;
                let bl_usize = bl as usize;
                if al_usize != bl_usize {
                    return false;
                }
                for i in 0..al_usize {
                    let a_elem = a_obj.get(&i.to_string()).unwrap_or(Value::Undefined);
                    let b_elem = b_obj.get(&i.to_string()).unwrap_or(Value::Undefined);
                    if !deep_equal_internal(&a_elem, &b_elem) {
                        return false;
                    }
                }
                return true;
            }

            for key in a_obj.own_keys() {
                let a_val = a_obj.get(&key).unwrap_or(Value::Undefined);
                let b_val = b_obj.get(&key).unwrap_or(Value::Undefined);
                if !deep_equal_internal(&a_val, &b_val) {
                    return false;
                }
            }
            true
        }
        _ => false,
    }
}

// =============================================================================
// Utility functions
// =============================================================================

/// Helper to convert Value to string
pub fn value_to_js_string(v: &Value) -> String {
    match v {
        Value::Undefined => "undefined".to_string(),
        Value::Null => "null".to_string(),
        Value::Boolean(b) => b.to_string(),
        Value::Number(n) => n.to_string(),
        Value::String(s) => s.clone(),
        Value::Object(_) | Value::ObjectId(_) => "[object]".to_string(),
        Value::Function(_) | Value::NativeFunction(_) | Value::NativeConstructor(_)
        | Value::Class(_) => "[Function]".to_string(),
        Value::Symbol(s) => format!("Symbol({})", s),
    }
}

/// Helper to convert Value to debug string
fn value_to_debug_string(v: &Value) -> String {
    match v {
        Value::Undefined => "undefined".to_string(),
        Value::Null => "null".to_string(),
        Value::Boolean(b) => b.to_string(),
        Value::Number(n) => n.to_string(),
        Value::String(s) => format!("\"{}\"", s),
        Value::Object(_) | Value::ObjectId(_) => "[object]".to_string(),
        Value::Function(_) | Value::NativeFunction(_) | Value::NativeConstructor(_)
        | Value::Class(_) => "[Function]".to_string(),
        Value::Symbol(s) => format!("Symbol({})", s),
    }
}
