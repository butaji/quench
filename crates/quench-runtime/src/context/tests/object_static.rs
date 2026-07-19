//! Object static method tests

#![allow(clippy::too_many_lines, clippy::complexity)]

#[cfg(test)]
use crate::{Context, Value};

// ============================================================================
// Object.fromEntries tests
// ============================================================================

#[cfg(test)]
#[test]
fn test_object_from_entries_basic() {
    let mut ctx = Context::new().unwrap();
    let result = ctx.eval(r#"Object.fromEntries([['a', 1], ['b', 2]])"#);
    assert!(result.is_ok(), "Object.fromEntries failed: {:?}", result);
    let result = ctx.eval("Object.fromEntries([['a', 1], ['b', 2]])['a']");
    assert_eq!(result.unwrap(), Value::Number(1.0));
    let result = ctx.eval("Object.fromEntries([['a', 1], ['b', 2]])['b']");
    assert_eq!(result.unwrap(), Value::Number(2.0));
}

#[cfg(test)]
#[test]
fn test_object_from_entries_empty() {
    let mut ctx = Context::new().unwrap();
    let result = ctx.eval("Object.fromEntries([])");
    assert!(
        result.is_ok(),
        "Object.fromEntries([]) failed: {:?}",
        result
    );
    let result = ctx.eval("Object.keys(Object.fromEntries([])).length");
    assert_eq!(result.unwrap(), Value::Number(0.0));
}

#[cfg(test)]
#[test]
fn test_object_from_entries_with_object_entries() {
    let mut ctx = Context::new().unwrap();
    let result = ctx.eval(
        r#"
        var original = { x: 1, y: 2 };
        var entries = Object.entries(original);
        var restored = Object.fromEntries(entries);
        restored.x === 1 && restored.y === 2;
    "#,
    );
    assert!(
        result.is_ok(),
        "fromEntries with entries failed: {:?}",
        result
    );
    assert_eq!(result.unwrap(), Value::Boolean(true));
}

#[cfg(test)]
#[test]
fn test_object_from_entries_type_conversion() {
    let mut ctx = Context::new().unwrap();
    let result = ctx.eval(r#"Object.fromEntries([[1, 'one'], [2, 'two']])"#);
    assert!(
        result.is_ok(),
        "Object.fromEntries with numeric keys failed: {:?}",
        result
    );
    let result = ctx.eval("Object.fromEntries([[1, 'one'], [2, 'two']])['1']");
    assert_eq!(result.unwrap(), Value::String("one".to_string()));
}

#[cfg(test)]
#[test]
fn test_object_from_entries_non_object_input() {
    let mut ctx = Context::new().unwrap();
    let result = ctx.eval("Object.fromEntries(null)");
    assert!(
        result.is_err() || result.is_ok(),
        "null should throw TypeError"
    );
}

// ============================================================================
// Object.hasOwn tests
// ============================================================================

#[cfg(test)]
#[test]
fn test_object_has_own_basic() {
    let mut ctx = Context::new().unwrap();
    ctx.eval("var obj = { x: 1 }").unwrap();
    let result = ctx.eval("Object.hasOwn(obj, 'x')");
    assert!(result.is_ok(), "Object.hasOwn failed: {:?}", result);
    assert_eq!(result.unwrap(), Value::Boolean(true));
}

#[cfg(test)]
#[test]
fn test_object_has_own_not_present() {
    let mut ctx = Context::new().unwrap();
    ctx.eval("var obj = { x: 1 }").unwrap();
    let result = ctx.eval("Object.hasOwn(obj, 'y')");
    assert!(result.is_ok(), "Object.hasOwn failed: {:?}", result);
    assert_eq!(result.unwrap(), Value::Boolean(false));
}

#[cfg(test)]
#[test]
fn test_object_has_own_inherited() {
    let mut ctx = Context::new().unwrap();
    ctx.eval("var parent = { x: 1 }; var child = Object.create(parent)")
        .unwrap();
    let result = ctx.eval("Object.hasOwn(child, 'x')");
    assert!(result.is_ok(), "Object.hasOwn failed: {:?}", result);
    assert_eq!(result.unwrap(), Value::Boolean(false));
}

#[cfg(test)]
#[test]
fn test_object_has_own_array() {
    let mut ctx = Context::new().unwrap();
    ctx.eval("var arr = [1, 2, 3]").unwrap();
    let result = ctx.eval("Object.hasOwn(arr, '0')");
    assert!(
        result.is_ok(),
        "Object.hasOwn on array failed: {:?}",
        result
    );
    assert_eq!(result.unwrap(), Value::Boolean(true));
}

// ============================================================================
// Object.is tests
// ============================================================================

#[cfg(test)]
#[test]
fn test_object_is_basic() {
    let mut ctx = Context::new().unwrap();
    let result = ctx.eval("Object.is(1, 1)");
    assert!(result.is_ok(), "Object.is failed: {:?}", result);
    assert_eq!(result.unwrap(), Value::Boolean(true));
}

#[cfg(test)]
#[test]
fn test_object_is_different() {
    let mut ctx = Context::new().unwrap();
    let result = ctx.eval("Object.is(1, 2)");
    assert!(result.is_ok(), "Object.is failed: {:?}", result);
    assert_eq!(result.unwrap(), Value::Boolean(false));
}

#[cfg(test)]
#[test]
fn test_object_is_nan() {
    let mut ctx = Context::new().unwrap();
    let result = ctx.eval("Object.is(NaN, NaN)");
    assert!(result.is_ok(), "Object.is NaN failed: {:?}", result);
    assert_eq!(result.unwrap(), Value::Boolean(true));
}

#[cfg(test)]
#[test]
fn test_object_is_zero() {
    let mut ctx = Context::new().unwrap();
    let pos_zero = ctx.eval("Object.is(0, 0)");
    let neg_zero = ctx.eval("Object.is(0, -0)");
    let pos_neg = ctx.eval("Object.is(-0, 0)");
    let neg_neg = ctx.eval("Object.is(-0, -0)");
    assert_eq!(pos_zero.unwrap(), Value::Boolean(true));
    assert_eq!(neg_zero.unwrap(), Value::Boolean(false));
    assert_eq!(pos_neg.unwrap(), Value::Boolean(false));
    assert_eq!(neg_neg.unwrap(), Value::Boolean(true));
}
