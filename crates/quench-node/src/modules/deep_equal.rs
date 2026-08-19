//! Deep structural equality for the `assert` module.
//!
//! Primitives use `Object.is` semantics in strict mode and `==` in
//! legacy mode; objects compare own enumerable string-keyed
//! properties recursively. Prototype chains are compared loosely
//! (identity only through `same_value`), which is sufficient for the
//! host's conformance scope.

use std::rc::Rc;

use quench_runtime::execute::{self, VmError};
use quench_runtime::value::Value;

/// `deepStrictEqual` (strict) / `deepEqual` (legacy) comparison.
pub fn deep_equal(left: &Value, right: &Value, strict: bool) -> Result<bool, VmError> {
    let mut memo: Vec<(*const (), *const ())> = Vec::new();
    compare(left, right, strict, &mut memo)
}

fn compare(
    left: &Value,
    right: &Value,
    strict: bool,
    memo: &mut Vec<(*const (), *const ())>,
) -> Result<bool, VmError> {
    let left = &deref_alias(left);
    let right = &deref_alias(right);
    if execute::same_value(left, right) {
        return Ok(true);
    }
    match (left, right) {
        (Value::Array(_), Value::Array(_)) => compare_arrays(left, right, strict, memo),
        (Value::Object(_), Value::Object(_)) => compare_objects(left, right, strict, memo),
        _ if strict => Ok(false),
        _ => execute::abstract_equal(left, right),
    }
}

/// Object identity for cycle detection; `None` for non-containers.
fn deref_alias(value: &Value) -> Value {
    match value {
        Value::ObjectAlias(alias) => alias
            .0
            .borrow()
            .upgrade()
            .map(Value::Object)
            .unwrap_or_else(|| value.clone()),
        _ => value.clone(),
    }
}
fn identity(value: &Value) -> Option<*const ()> {
    match value {
        Value::Object(object) => Some(Rc::as_ptr(object).cast()),
        Value::Array(array) => Some(Rc::as_ptr(array).cast()),
        _ => None,
    }
}

fn seen(memo: &mut Vec<(*const (), *const ())>, left: &Value, right: &Value) -> bool {
    let (Some(a), Some(b)) = (identity(left), identity(right)) else {
        return false;
    };
    if memo.contains(&(a, b)) {
        return true;
    }
    memo.push((a, b));
    false
}

fn compare_arrays(
    left: &Value,
    right: &Value,
    strict: bool,
    memo: &mut Vec<(*const (), *const ())>,
) -> Result<bool, VmError> {
    let (Value::Array(a), Value::Array(b)) = (left, right) else {
        return Ok(false);
    };
    if a.logical_len() != b.logical_len() {
        return Ok(false);
    }
    if seen(memo, left, right) {
        return Ok(true);
    }
    for index in 0..a.logical_len() {
        let key = index.to_string();
        let la = execute::get_property_result(left, &key)?;
        let rb = execute::get_property_result(right, &key)?;
        if !compare(&la, &rb, strict, memo)? {
            return Ok(false);
        }
    }
    Ok(true)
}

fn compare_objects(
    left: &Value,
    right: &Value,
    strict: bool,
    memo: &mut Vec<(*const (), *const ())>,
) -> Result<bool, VmError> {
    if seen(memo, left, right) {
        return Ok(true);
    }
    let left_keys = execute::own_enumerable_keys(left);
    let right_keys = execute::own_enumerable_keys(right);
    if left_keys.len() != right_keys.len() || left_keys.iter().any(|key| !right_keys.contains(key))
    {
        return Ok(false);
    }
    for key in &left_keys {
        let la = execute::get_property_result(left, key)?;
        let rb = execute::get_property_result(right, key)?;
        if !compare(&la, &rb, strict, memo)? {
            return Ok(false);
        }
    }
    Ok(true)
}
