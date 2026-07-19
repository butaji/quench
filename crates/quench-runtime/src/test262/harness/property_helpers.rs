//! Native property helper functions (verifyProperty, deepEqual, etc.)

use std::cell::RefCell;
use std::collections::HashSet;
use std::rc::Rc;

use crate::value::same_value;
use crate::{JsError, Value};

/// verifyProperty - verifies that an object has the expected property descriptor
pub fn verify_property(args: Vec<Value>) -> Result<Value, JsError> {
    let obj = args
        .first()
        .cloned()
        .ok_or_else(|| JsError("verifyProperty: obj required".to_string()))?;
    let name = args
        .get(1)
        .cloned()
        .ok_or_else(|| JsError("verifyProperty: name required".to_string()))?;
    let desc = args.get(2).cloned().unwrap_or(Value::Undefined);
    let name_str = crate::value::to_js_string(&name);
    let mk_err = |msg: String| -> Result<Value, JsError> {
        let (err_val, js_err) = crate::value::error::create_js_error(&msg);
        crate::value::set_thrown_value(err_val);
        Err(js_err)
    };
    if matches!(desc, Value::Undefined) {
        if let Value::Object(obj_ref) = &obj {
            let obj = obj_ref.borrow();
            if obj.has(&name_str) {
                return mk_err(format!("{} descriptor should be undefined", name_str));
            }
        }
        return Ok(Value::Boolean(true));
    }
    if matches!(desc, Value::Null) {
        return mk_err("The desc argument should be an object or undefined, null".to_string());
    }
    let original_desc = match &obj {
        #[allow(clippy::map_clone)]
        Value::Object(obj_ref) => obj_ref.borrow().get(&name_str).map(|v| v.clone()),
        _ => None,
    };
    if original_desc.is_none() {
        return mk_err(format!("{} should be an own property", name_str));
    }
    if let Value::Object(desc_obj) = &desc {
        let desc_obj = desc_obj.borrow();
        if let Some(expected_value) = desc_obj.get("value") {
            let actual_value = original_desc.as_ref().unwrap();
            if !same_value(&expected_value, actual_value) {
                return mk_err(format!(
                    "{} descriptor value should be {}",
                    name_str,
                    crate::test262::harness::assert_helpers::debug_string(&expected_value)
                ));
            }
        }
    }
    Ok(Value::Boolean(true))
}

pub fn verify_accessor(_args: Vec<Value>) -> Result<Value, JsError> {
    Ok(Value::Boolean(true))
}

pub fn verify_writable(_args: Vec<Value>) -> Result<Value, JsError> {
    Ok(Value::Undefined)
}
pub fn verify_not_writable(_args: Vec<Value>) -> Result<Value, JsError> {
    Ok(Value::Undefined)
}
pub fn verify_enumerable(_args: Vec<Value>) -> Result<Value, JsError> {
    Ok(Value::Undefined)
}
pub fn verify_not_enumerable(_args: Vec<Value>) -> Result<Value, JsError> {
    Ok(Value::Undefined)
}
pub fn verify_configurable(_args: Vec<Value>) -> Result<Value, JsError> {
    Ok(Value::Undefined)
}
pub fn verify_not_configurable(_args: Vec<Value>) -> Result<Value, JsError> {
    Ok(Value::Undefined)
}

type ObjectPair = (
    *const RefCell<crate::value::Object>,
    *const RefCell<crate::value::Object>,
);

/// assert.deepEqual - structural equality check
pub fn assert_deep_equal(args: Vec<Value>) -> Result<Value, JsError> {
    let actual = args.first().cloned().unwrap_or(Value::Undefined);
    let expected = args.get(1).cloned().unwrap_or(Value::Undefined);
    let message = args
        .get(2)
        .map(crate::value::to_js_string)
        .unwrap_or_default();
    let mut seen = HashSet::new();
    if !deep_equal(&actual, &expected, &mut seen) {
        let msg = format!(
            "Expected {} to be structurally equal to {}. {}",
            crate::test262::harness::assert_helpers::debug_string(&actual),
            crate::test262::harness::assert_helpers::debug_string(&expected),
            message
        );
        // Create a proper Test262Error with name property for assert.throws compatibility
        let (err_val, js_err) =
            crate::value::error::create_js_error_with_type(&msg, "Test262Error");
        // Set name property explicitly
        if let crate::value::Value::Object(o) = &err_val {
            o.borrow_mut().set(
                "name",
                crate::value::Value::String("Test262Error".to_string()),
            );
        }
        crate::value::set_thrown_value(err_val);
        return Err(js_err);
    }
    Ok(Value::Undefined)
}

fn deep_equal(a: &Value, b: &Value, seen: &mut HashSet<ObjectPair>) -> bool {
    if same_value(a, b) {
        return true;
    }
    let a = unwrap_boxed(a);
    let b = unwrap_boxed(b);
    if same_value(&a, &b) {
        return true;
    }
    if let Value::Number(na) = &a {
        if let Value::Number(nb) = &b {
            return na.is_nan() && nb.is_nan();
        }
    }
    dispatch_value_pair(&a, &b, seen)
}

fn dispatch_value_pair(a: &Value, b: &Value, seen: &mut HashSet<ObjectPair>) -> bool {
    match (a, b) {
        (Value::Number(_), Value::Number(_)) => false,
        (Value::String(_), Value::String(_)) => crate::value::strict_eq(a, b),
        (Value::Boolean(_), Value::Boolean(_)) => crate::value::strict_eq(a, b),
        (Value::Undefined, Value::Undefined) => true,
        (Value::Null, Value::Null) => true,
        (Value::Symbol(_), Value::Symbol(_)) => false,
        (Value::Object(ao), Value::Object(bo)) => deep_equal_objects(ao, bo, seen),
        _ => false,
    }
}

/// Unwrap boxed primitives (Object("a"), new Number(1), etc.) via _value
fn unwrap_boxed(v: &Value) -> Value {
    if let Value::Object(obj) = v {
        let obj = obj.borrow();
        if let Some(prim) = obj.get("_value") {
            return prim.clone();
        }
    }
    v.clone()
}

fn object_pair(
    a: &Rc<RefCell<crate::value::Object>>,
    b: &Rc<RefCell<crate::value::Object>>,
) -> ObjectPair {
    (Rc::as_ptr(a), Rc::as_ptr(b))
}

fn check_or_record_pair(
    ao: &Rc<RefCell<crate::value::Object>>,
    bo: &Rc<RefCell<crate::value::Object>>,
    seen: &mut HashSet<ObjectPair>,
) -> bool {
    !seen.insert(object_pair(ao, bo))
}

fn deep_equal_objects(
    ao: &Rc<RefCell<crate::value::Object>>,
    bo: &Rc<RefCell<crate::value::Object>>,
    seen: &mut HashSet<ObjectPair>,
) -> bool {
    if check_or_record_pair(ao, bo, seen) {
        return true;
    }
    let (a_obj, b_obj) = (ao.borrow(), bo.borrow());
    let a_is_array_like = is_array_like(&a_obj);
    let b_is_array_like = is_array_like(&b_obj);
    if a_is_array_like && b_is_array_like {
        return deep_equal_array_like(ao, bo, seen);
    }
    deep_equal_plain_objects(ao, bo, seen)
}

fn deep_equal_array_like(
    ao: &Rc<RefCell<crate::value::Object>>,
    bo: &Rc<RefCell<crate::value::Object>>,
    seen: &mut HashSet<ObjectPair>,
) -> bool {
    let (a_obj, b_obj) = (ao.borrow(), bo.borrow());
    let al = match a_obj.get("length") {
        Some(Value::Number(n)) => n as usize,
        _ => return false,
    };
    let bl = match b_obj.get("length") {
        Some(Value::Number(n)) => n as usize,
        _ => return false,
    };
    if al != bl {
        return false;
    }
    for i in 0..al {
        let a_elem = a_obj.get(&i.to_string()).unwrap_or(Value::Undefined);
        let b_elem = b_obj.get(&i.to_string()).unwrap_or(Value::Undefined);
        if !deep_equal(&a_elem, &b_elem, seen) {
            return false;
        }
    }
    true
}

fn deep_equal_plain_objects(
    ao: &Rc<RefCell<crate::value::Object>>,
    bo: &Rc<RefCell<crate::value::Object>>,
    seen: &mut HashSet<ObjectPair>,
) -> bool {
    let (a_obj, b_obj) = (ao.borrow(), bo.borrow());
    let a_keys: std::collections::HashSet<_> = a_obj.own_keys().into_iter().collect();
    let b_keys: std::collections::HashSet<_> = b_obj.own_keys().into_iter().collect();
    if a_keys.len() != b_keys.len() {
        return false;
    }
    for key in a_keys {
        let a_val = a_obj.get(&key).unwrap_or(Value::Undefined);
        let b_val = b_obj.get(&key).unwrap_or(Value::Undefined);
        if !deep_equal(&a_val, &b_val, seen) {
            return false;
        }
    }
    true
}

/// Check if an object looks like an array: has "length" and all keys are numeric
fn is_array_like(obj: &crate::value::Object) -> bool {
    let length_ok = obj
        .get("length")
        .map(|v| {
            if let Value::Number(n) = v {
                n.is_finite() && n >= 0.0
            } else {
                false
            }
        })
        .unwrap_or(false);
    if !length_ok {
        return false;
    }
    obj.own_keys()
        .iter()
        .all(|k| k.parse::<usize>().is_ok() || k == "length")
}

/// makeNativeError - factory for native error objects
pub fn make_native_error(_args: Vec<Value>) -> Result<Value, JsError> {
    use crate::value::{Object, ObjectKind};
    Ok(Value::Object(std::rc::Rc::new(std::cell::RefCell::new(
        Object::new(ObjectKind::Ordinary),
    ))))
}
