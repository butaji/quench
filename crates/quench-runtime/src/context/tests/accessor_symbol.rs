//! Tests for Symbol-keyed accessor properties — getter/setter identity.

use crate::{Context, Value};

/// Getter shorthand: getOwnPropertyDescriptor.get must be the getter function
/// (desc.get returns the RESULT of calling the getter, not the function itself)
#[test]
fn test_getter_shorthand_define_property_identity() {
    let mut ctx = Context::new().unwrap();
    ctx.eval("var sym = Symbol('test')").unwrap();
    ctx.eval("var desc = { enumerable: true, configurable: true, get() { return 42; }, set() {} }")
        .unwrap();

    // Define property using getter shorthand descriptor
    ctx.eval("var obj = {}").unwrap();
    ctx.eval("Object.defineProperty(obj, sym, desc)").unwrap();

    // getOwnPropertyDescriptor(obj, sym).get must be a function
    let getter_is_fn =
        ctx.eval("typeof Object.getOwnPropertyDescriptor(obj, sym).get === 'function'");
    assert_eq!(
        getter_is_fn.unwrap(),
        Value::Boolean(true),
        "getOwnPropertyDescriptor.get must be a function"
    );

    // getOwnPropertyDescriptor(obj, sym).set must be a function
    let setter_is_fn =
        ctx.eval("typeof Object.getOwnPropertyDescriptor(obj, sym).set === 'function'");
    assert_eq!(
        setter_is_fn.unwrap(),
        Value::Boolean(true),
        "getOwnPropertyDescriptor.set must be a function"
    );

    // Calling the getter must return 42
    let getter_value = ctx.eval("Object.getOwnPropertyDescriptor(obj, sym).get.call(obj)");
    assert_eq!(
        getter_value.unwrap(),
        Value::Number(42.0),
        "getter must return 42"
    );
}

/// Getter shorthand: getter must return correct value when called
#[test]
fn test_getter_shorthand_define_property_value() {
    let mut ctx = Context::new().unwrap();
    ctx.eval("var sym = Symbol('test')").unwrap();
    ctx.eval("var desc = { enumerable: true, configurable: true, get() { return 42; }, set() {} }")
        .unwrap();
    ctx.eval("var obj = {}").unwrap();
    ctx.eval("Object.defineProperty(obj, sym, desc)").unwrap();

    let result = ctx.eval("obj[sym]");
    assert_eq!(result.unwrap(), Value::Number(42.0));
}

/// Getter shorthand: setter must work correctly
#[test]
fn test_setter_shorthand_define_property() {
    let mut ctx = Context::new().unwrap();
    ctx.eval("var sym = Symbol('test')").unwrap();
    ctx.eval("var stored = null").unwrap();
    ctx.eval(
        r#"var desc = {
            enumerable: true,
            configurable: true,
            get() { return stored; },
            set(v) { stored = v; }
        }"#,
    )
    .unwrap();
    ctx.eval("var obj = {}").unwrap();
    ctx.eval("Object.defineProperty(obj, sym, desc)").unwrap();

    // Set via the accessor
    ctx.eval("obj[sym] = 99").unwrap();
    let result = ctx.eval("obj[sym]");
    assert_eq!(result.unwrap(), Value::Number(99.0));
}

/// Getter data style ({ get: fn }): getOwnPropertyDescriptor.get must be same function
#[test]
fn test_getter_data_style_define_property_identity() {
    let mut ctx = Context::new().unwrap();
    ctx.eval("var sym = Symbol('test')").unwrap();
    ctx.eval("var fn = function() { return 42; }").unwrap();
    ctx.eval("var desc = { enumerable: true, configurable: true, get: fn }")
        .unwrap();
    ctx.eval("var obj = {}").unwrap();
    ctx.eval("Object.defineProperty(obj, sym, desc)").unwrap();

    // For { get: fn } style, desc.get IS the function (data property "get" with value fn)
    // getOwnPropertyDescriptor returns the same function
    let desc_d = ctx.eval("Object.getOwnPropertyDescriptor(obj, sym)");
    let desc_get = ctx.eval("Object.getOwnPropertyDescriptor(obj, sym).get");
    let is_same = ctx.eval("Object.is(Object.getOwnPropertyDescriptor(obj, sym).get, fn)");

    // Diagnose: what is the descriptor?
    let has_get = ctx.eval("Object.getOwnPropertyDescriptor(obj, sym).get !== undefined");
    let desc_keys = ctx.eval("Object.keys(Object.getOwnPropertyDescriptor(obj, sym))");

    // Check if property exists
    let has_prop = ctx.eval("Object.hasOwn(obj, sym)");
    let obj_keys = ctx.eval("Object.keys(obj)");

    eprintln!("DEBUG: desc_d = {:?}", desc_d);
    eprintln!("DEBUG: desc_keys = {:?}", desc_keys);
    eprintln!("DEBUG: has_prop = {:?}", has_prop);
    eprintln!("DEBUG: obj_keys = {:?}", obj_keys);
    eprintln!("DEBUG: desc_get = {:?}", desc_get);
    eprintln!("DEBUG: has_get = {:?}", has_get);
    // Check if it's a data descriptor
    let desc_value = ctx.eval("Object.getOwnPropertyDescriptor(obj, sym).value");
    eprintln!("DEBUG: desc_value = {:?}", desc_value);
    let desc_str = ctx.eval("JSON.stringify(Object.getOwnPropertyDescriptor(obj, sym))");
    eprintln!("DEBUG: desc_str = {:?}", desc_str);
    // Check desc object itself
    let desc_has_get = ctx.eval("'get' in desc");
    eprintln!("DEBUG: desc_has_get = {:?}", desc_has_get);
    // Check what value the property holds
    let prop_val = ctx.eval("obj[sym]");
    eprintln!("DEBUG: obj[sym] = {:?}", prop_val);
    // Check flags in descriptor
    let has_enumerable = ctx.eval("Object.getOwnPropertyDescriptor(obj, sym).enumerable");
    let has_configurable = ctx.eval("Object.getOwnPropertyDescriptor(obj, sym).configurable");
    let has_writable = ctx.eval("Object.getOwnPropertyDescriptor(obj, sym).writable");
    eprintln!("DEBUG: enumerable = {:?}", has_enumerable);
    eprintln!("DEBUG: configurable = {:?}", has_configurable);
    eprintln!("DEBUG: writable = {:?}", has_writable);
    let all_keys = ctx.eval("Reflect.ownKeys(Object.getOwnPropertyDescriptor(obj, sym))");
    eprintln!("DEBUG: all_keys = {:?}", all_keys);

    assert_eq!(
        is_same.unwrap(),
        Value::Boolean(true),
        "getter function identity must be preserved for get:fn style"
    );
}

/// hasOwnProperty for Symbol-keyed accessor must return true
#[test]
fn test_has_own_property_symbol_keyed_accessor() {
    let mut ctx = Context::new().unwrap();
    ctx.eval("var sym = Symbol('test')").unwrap();
    ctx.eval("var desc = { get() { return 42; }, set() {} }")
        .unwrap();
    ctx.eval("var obj = {}").unwrap();
    ctx.eval("Object.defineProperty(obj, sym, desc)").unwrap();

    let result = ctx.eval("Object.prototype.hasOwnProperty.call(obj, sym)");
    assert_eq!(result.unwrap(), Value::Boolean(true));
}

/// hasOwn for Symbol-keyed accessor must return true
#[test]
fn test_has_own_symbol_keyed_accessor() {
    let mut ctx = Context::new().unwrap();
    ctx.eval("var sym = Symbol('test')").unwrap();
    ctx.eval("var desc = { get() { return 42; }, set() {} }")
        .unwrap();
    ctx.eval("var obj = {}").unwrap();
    ctx.eval("Object.defineProperty(obj, sym, desc)").unwrap();

    let result = ctx.eval("Object.hasOwn(obj, sym)");
    assert_eq!(result.unwrap(), Value::Boolean(true));
}

/// Symbol-keyed accessor: propertyIsEnumerable must return true
#[test]
fn test_property_is_enumerable_symbol_keyed_accessor() {
    let mut ctx = Context::new().unwrap();
    ctx.eval("var sym = Symbol('test')").unwrap();
    ctx.eval("var desc = { enumerable: true, configurable: true, get() { return 42; } }")
        .unwrap();
    ctx.eval("var obj = {}").unwrap();
    ctx.eval("Object.defineProperty(obj, sym, desc)").unwrap();

    let result = ctx.eval("Object.prototype.propertyIsEnumerable.call(obj, sym)");
    assert_eq!(result.unwrap(), Value::Boolean(true));
}

/// getOwnPropertyDescriptor for Symbol-keyed accessor must return correct descriptor
#[test]
fn test_get_own_property_descriptor_symbol_keyed_accessor() {
    let mut ctx = Context::new().unwrap();
    ctx.eval("var sym = Symbol('test')").unwrap();
    ctx.eval(
        "var desc = { enumerable: false, configurable: true, get() { return 42; }, set(v) {} }",
    )
    .unwrap();
    ctx.eval("var obj = {}").unwrap();
    ctx.eval("Object.defineProperty(obj, sym, desc)").unwrap();

    let result = ctx.eval(
        r#"
        var d = Object.getOwnPropertyDescriptor(obj, sym);
        d.get !== undefined && d.set !== undefined && d.enumerable === false && d.configurable === true
        "#,
    );
    assert_eq!(result.unwrap(), Value::Boolean(true));
}

/// getOwnPropertyDescriptor for Symbol-keyed accessor without setter
#[test]
fn test_get_own_property_descriptor_symbol_no_setter() {
    let mut ctx = Context::new().unwrap();
    ctx.eval("var sym = Symbol('test')").unwrap();
    ctx.eval("var desc = { enumerable: true, configurable: false, get() { return 99; } }")
        .unwrap();
    ctx.eval("var obj = {}").unwrap();
    ctx.eval("Object.defineProperty(obj, sym, desc)").unwrap();

    let result = ctx.eval(
        r#"
        var d = Object.getOwnPropertyDescriptor(obj, sym);
        d.get !== undefined && d.set === undefined && d.enumerable === true && d.configurable === false
        "#,
    );
    assert_eq!(result.unwrap(), Value::Boolean(true));
}

/// Multiple Symbol-keyed accessor properties
#[test]
fn test_multiple_symbol_keyed_accessors() {
    let mut ctx = Context::new().unwrap();
    ctx.eval("var sym1 = Symbol('a'); var sym2 = Symbol('b')")
        .unwrap();
    ctx.eval("var obj = {}").unwrap();
    ctx.eval("Object.defineProperty(obj, sym1, { get() { return 1; } })")
        .unwrap();
    ctx.eval("Object.defineProperty(obj, sym2, { get() { return 2; } })")
        .unwrap();

    let r1 = ctx.eval("obj[sym1]");
    let r2 = ctx.eval("obj[sym2]");
    assert_eq!(r1.unwrap(), Value::Number(1.0));
    assert_eq!(r2.unwrap(), Value::Number(2.0));
}
