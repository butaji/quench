//! Native assert.deepEqual - structural equality check.
//!
//! Split out of property_helpers.rs to satisfy the 500-line module limit.

use std::cell::RefCell;
use std::collections::HashSet;
use std::rc::Rc;

use crate::value::same_value;
use crate::{JsError, Value};

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
    // Official deepEqual uses Reflect.ownKeys: Symbol-keyed own properties
    // participate in the comparison.
    if !deep_equal_symbol_props(&a_obj, &b_obj, seen) {
        return false;
    }
    let a_is_array_like = is_array_like(&a_obj);
    let b_is_array_like = is_array_like(&b_obj);
    if a_is_array_like && b_is_array_like {
        return deep_equal_array_like(ao, bo, seen);
    }
    deep_equal_plain_objects(ao, bo, seen)
}

/// Compare Symbol-keyed own properties. Symbol keys live either in
/// symbol_properties or in properties under the raw `desc\0id` payload key
/// (which own_keys filters out), so both stores are collected here.
fn deep_equal_symbol_props(
    a_obj: &crate::value::Object,
    b_obj: &crate::value::Object,
    seen: &mut HashSet<ObjectPair>,
) -> bool {
    let collect = |o: &crate::value::Object| -> std::collections::HashMap<String, Value> {
        let mut m: std::collections::HashMap<String, Value> = o
            .symbol_properties
            .iter()
            .map(|(k, v)| (k.clone(), v.clone()))
            .collect();
        for (k, v) in o.properties.iter() {
            if k.contains('\0') {
                m.insert(k.clone(), v.clone());
            }
        }
        m
    };
    let a_syms = collect(a_obj);
    let b_syms = collect(b_obj);
    if a_syms.len() != b_syms.len() {
        return false;
    }
    a_syms
        .iter()
        .all(|(k, av)| b_syms.get(k).is_some_and(|bv| deep_equal(av, bv, seen)))
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

#[cfg(test)]
mod tests {
    use crate::test262::harness::try_inject_harness;

    fn harness_ctx() -> crate::Context {
        let mut ctx = crate::Context::new().unwrap();
        try_inject_harness(&mut ctx).unwrap();
        ctx
    }

    #[test]
    fn test_deep_equal_symbol_key_value_mismatch_throws() {
        // Official deepEqual uses Reflect.ownKeys: Symbol-keyed own properties
        // participate in the comparison.
        let mut ctx = harness_ctx();
        let result = ctx.eval(
            "var sym = Symbol('k'); var a = {}; a[sym] = 1; var b = {}; b[sym] = 2; assert.deepEqual(a, b);",
        );
        assert!(
            result.is_err(),
            "same symbol key with different values must not be deep-equal: {:?}",
            result
        );
    }

    #[test]
    fn test_deep_equal_symbol_key_same_value_passes() {
        let mut ctx = harness_ctx();
        let result = ctx.eval(
            "var sym = Symbol('k'); var a = {}; a[sym] = 1; var b = {}; b[sym] = 1; assert.deepEqual(a, b);",
        );
        assert!(
            result.is_ok(),
            "same symbol key with same value should be deep-equal: {:?}",
            result
        );
    }

    #[test]
    fn test_deep_equal_different_symbol_keys_throw() {
        // Distinct symbols (same description) are distinct keys.
        let mut ctx = harness_ctx();
        let result = ctx.eval(
            "var a = {}; a[Symbol('k')] = 1; var b = {}; b[Symbol('k')] = 1; assert.deepEqual(a, b);",
        );
        assert!(
            result.is_err(),
            "different symbol keys must not be deep-equal: {:?}",
            result
        );
    }
}
