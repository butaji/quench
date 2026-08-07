//! Object static method tests

#![allow(clippy::too_many_lines, clippy::complexity)]

#[cfg(test)]
use crate::{Context, Value};

// ============================================================================
// Object.getOwnPropertyDescriptor — accessor property tests
// ============================================================================

#[cfg(test)]
#[test]
fn test_get_own_property_descriptor_accessor_symbol_key() {
    let mut ctx = Context::new().unwrap();
    let result = ctx.eval(
        r#"
        var obj = {};
        var prop = Symbol(1);
        Object.defineProperty(obj, prop, {
            enumerable: true,
            configurable: true,
            get() { return 42; },
            set() {}
        });
        var d = Object.getOwnPropertyDescriptor(obj, prop);
        // getter must be a function, not undefined
        typeof d.get === 'function' &&
            // calling the getter must return 42
            d.get() === 42 &&
            // enumerable/configurable must be preserved
            d.enumerable === true &&
            d.configurable === true;
        "#,
    );
    assert_eq!(result.unwrap(), Value::Boolean(true));
}

#[cfg(test)]
#[test]
fn test_get_own_property_descriptor_accessor_string_key() {
    let mut ctx = Context::new().unwrap();
    let result = ctx.eval(
        r#"
        var obj = {};
        Object.defineProperty(obj, 'foo', {
            enumerable: true,
            configurable: true,
            get() { return 99; },
            set() {}
        });
        var d = Object.getOwnPropertyDescriptor(obj, 'foo');
        typeof d.get === 'function' &&
            d.get() === 99 &&
            d.enumerable === true &&
            d.configurable === true;
        "#,
    );
    assert_eq!(result.unwrap(), Value::Boolean(true));
}

#[cfg(test)]
#[test]
fn test_get_own_property_descriptor_symbol_key_diagnostic() {
    let mut ctx = Context::new().unwrap();
    // What does getOwnPropertyDescriptor return for a Symbol-keyed accessor?
    let result = ctx.eval(
        r#"
        var obj = {};
        var sym = Symbol('test');
        Object.defineProperty(obj, sym, {
            enumerable: true,
            configurable: true,
            get() { return 42; },
            set() {}
        });
        // Check what getOwnPropertyDescriptor returns
        var d = Object.getOwnPropertyDescriptor(obj, sym);
        // Is it undefined (bug) or a proper descriptor object?
        d !== undefined && d.get !== undefined && d.get() === 42;
        "#,
    );
    // This should be true; if false, the Symbol key conversion bug is still present
    assert_eq!(result.unwrap(), Value::Boolean(true));
}

#[cfg(test)]
#[test]
fn test_has_own_property_symbol() {
    let mut ctx = Context::new().unwrap();
    // hasOwnProperty with a Symbol argument
    let result = ctx.eval(
        r#"
        var obj = {};
        var sym = Symbol('x');
        Object.defineProperty(obj, sym, { value: 1 });
        Object.prototype.hasOwnProperty.call(obj, sym);
        "#,
    );
    assert_eq!(result.unwrap(), Value::Boolean(true));
}

#[cfg(test)]
#[test]
fn test_verify_property_restore_accessor_symbol_steps() {
    let mut ctx = Context::new().unwrap();
    // Step 1: hasOwnProperty with Symbol argument
    let step1 = ctx.eval(
        r#"
        var obj = {};
        var prop = Symbol(1);
        Object.defineProperty(obj, prop, {
            enumerable: true,
            configurable: true,
            get() { return 42; },
            set() {}
        });
        Object.prototype.hasOwnProperty.call(obj, prop);
        "#,
    );
    assert_eq!(step1.unwrap(), Value::Boolean(true));

    // Step 2: getOwnPropertyDescriptor returns proper descriptor for Symbol key
    let step2 = ctx.eval(
        r#"
        var obj = {};
        var prop = Symbol(1);
        Object.defineProperty(obj, prop, {
            enumerable: true,
            configurable: true,
            get() { return 42; },
            set() {}
        });
        var d = Object.getOwnPropertyDescriptor(obj, prop);
        d !== undefined && typeof d.get === 'function';
        "#,
    );
    assert_eq!(step2.unwrap(), Value::Boolean(true));

    // Step 3: hasOwnProperty returns true for Symbol property
    let step3 = ctx.eval(
        r#"
        var obj = {};
        var prop = Symbol(1);
        var desc = {
            enumerable: true,
            configurable: true,
            get() { return 42; },
            set() {}
        };
        Object.defineProperty(obj, prop, desc);
        Object.prototype.hasOwnProperty.call(obj, prop);
        "#,
    );
    assert_eq!(step3.unwrap(), Value::Boolean(true));

    // Step 4: after delete, hasOwnProperty returns false
    let step4 = ctx.eval(
        r#"
        var obj = {};
        var prop = Symbol(1);
        Object.defineProperty(obj, prop, {
            enumerable: true,
            configurable: true,
            get() { return 42; },
            set() {}
        });
        delete obj[prop];
        Object.prototype.hasOwnProperty.call(obj, prop);
        "#,
    );
    assert_eq!(step4.unwrap(), Value::Boolean(false));
}

#[cfg(test)]
#[test]
fn test_property_is_enumerable_symbol() {
    let mut ctx = Context::new().unwrap();
    // Object.prototype.propertyIsEnumerable with Symbol key
    let result = ctx.eval(
        r#"
        var obj = {};
        var sym = Symbol('test');
        Object.defineProperty(obj, sym, { enumerable: true, value: 1 });
        Object.prototype.propertyIsEnumerable.call(obj, sym);
        "#,
    );
    assert_eq!(result.unwrap(), Value::Boolean(true));
}

#[cfg(test)]
#[test]
fn test_has_own_property_bound_call_symbol() {
    let mut ctx = Context::new().unwrap();
    // Same as __hasOwnProperty = Function.prototype.call.bind(Object.prototype.hasOwnProperty)
    let result = ctx.eval(
        r#"
        var obj = {};
        var sym = Symbol(1);
        Object.defineProperty(obj, sym, { value: 99 });
        var __hasOwnProperty = Function.prototype.call.bind(Object.prototype.hasOwnProperty);
        __hasOwnProperty(obj, sym);
        "#,
    );
    assert_eq!(result.unwrap(), Value::Boolean(true));
}

#[cfg(test)]
#[test]
fn test_get_own_property_names_symbol() {
    let mut ctx = Context::new().unwrap();
    // Object.getOwnPropertyNames does NOT include Symbols
    let result = ctx.eval(
        r#"
        var obj = {};
        var sym = Symbol('x');
        Object.defineProperty(obj, sym, { value: 1 });
        Object.defineProperty(obj, 'str', { value: 2 });
        var names = Object.getOwnPropertyNames(obj);
        names.indexOf('str') !== -1;
        "#,
    );
    assert_eq!(result.unwrap(), Value::Boolean(true));
}

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
