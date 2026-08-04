//! Class instance and static field tests

#![allow(clippy::too_many_lines, clippy::complexity)]

#[cfg(test)]
use crate::{Context, Value};

// ============================================================================
// Class instance fields tests
// ============================================================================

#[cfg(test)]
#[test]
fn test_class_instance_field_basic_isolated() {
    let mut ctx = Context::new().unwrap();
    ctx.eval("class C { x = 1; } C").unwrap();
}

#[cfg(test)]
#[test]
fn test_class_new_with_fields_isolated() {
    let mut ctx = Context::new().unwrap();
    ctx.eval("class C { x = 1; } new C()").unwrap();
}

#[cfg(test)]
#[test]
fn test_class_explicit_empty_constructor() {
    let mut ctx = Context::new().unwrap();
    ctx.eval("class C { x = 1; constructor() {} } new C()")
        .unwrap();
}

#[cfg(test)]
#[test]
fn test_class_parse_with_field() {
    let ctx = Context::new().unwrap();
    ctx.parse("class C { x = 1; }").unwrap();
}

#[cfg(test)]
#[test]
fn test_class_new_no_fields() {
    let mut ctx = Context::new().unwrap();
    ctx.eval("class C { } new C()").unwrap();
}

#[cfg(test)]
#[test]
fn test_class_instance_field_expr() {
    let mut ctx = Context::new().unwrap();
    let result = ctx.eval("class C { x = 2 + 3; } let c = new C(); c.x");
    assert!(result.is_ok(), "Instance field expr failed: {:?}", result);
    assert_eq!(result.unwrap(), Value::Number(5.0));
}

#[cfg(test)]
#[test]
fn test_class_instance_field_multiple() {
    let mut ctx = Context::new().unwrap();
    let result = ctx.eval("class C { x = 10; y = 20; } let c = new C(); c.x + c.y");
    assert!(result.is_ok(), "Multiple fields failed: {:?}", result);
    assert_eq!(result.unwrap(), Value::Number(30.0));
}

#[cfg(test)]
#[test]
fn test_class_instance_field_no_init() {
    let mut ctx = Context::new().unwrap();
    let result = ctx.eval("class C { x; } let c = new C(); c.x");
    assert!(result.is_ok(), "Field without init failed: {:?}", result);
    assert_eq!(result.unwrap(), Value::Undefined);
}

#[cfg(test)]
#[test]
fn test_class_instance_field_in_constructor() {
    let mut ctx = Context::new().unwrap();
    let result = ctx.eval("class C { x = 1; constructor() { return this; } } let c = new C(); c.x");
    assert!(
        result.is_ok(),
        "Field with explicit constructor failed: {:?}",
        result
    );
    assert_eq!(result.unwrap(), Value::Number(1.0));
}

// ============================================================================
// Class static fields tests
// ============================================================================

#[cfg(test)]
#[test]
fn test_class_static_field_basic() {
    let mut ctx = Context::new().unwrap();
    let result = ctx.eval("class C { static x = 42; } C.x");
    assert!(result.is_ok(), "Static field test failed: {:?}", result);
    assert_eq!(result.unwrap(), Value::Number(42.0));
}

#[cfg(test)]
#[test]
fn test_class_static_field_expr() {
    let mut ctx = Context::new().unwrap();
    let result = ctx.eval("class C { static x = 7 * 6; } C.x");
    assert!(result.is_ok(), "Static field expr failed: {:?}", result);
    assert_eq!(result.unwrap(), Value::Number(42.0));
}

#[cfg(test)]
#[test]
fn test_class_static_field_multiple() {
    let mut ctx = Context::new().unwrap();
    let result = ctx.eval("class C { static x = 1; static y = 2; } C.x + C.y");
    assert!(
        result.is_ok(),
        "Multiple static fields failed: {:?}",
        result
    );
    assert_eq!(result.unwrap(), Value::Number(3.0));
}

#[cfg(test)]
#[test]
fn test_class_static_field_no_init() {
    let mut ctx = Context::new().unwrap();
    let result = ctx.eval("class C { static x; } C.x");
    assert!(
        result.is_ok(),
        "Static field without init failed: {:?}",
        result
    );
    assert_eq!(result.unwrap(), Value::Undefined);
}

// ============================================================================
// Class instance + static fields combined tests
// ============================================================================

#[cfg(test)]
#[test]
fn test_class_instance_and_static_fields() {
    let mut ctx = Context::new().unwrap();
    let result = ctx.eval("class C { static x = 10; y = 20; } let c = new C(); C.x + c.y");
    assert!(result.is_ok(), "Combined fields failed: {:?}", result);
    assert_eq!(result.unwrap(), Value::Number(30.0));
}

#[test]
fn test_class_static_fields_are_own_properties() {
    let mut ctx = Context::new().unwrap();
    let result = ctx.eval(
        "class C { static value = 1; static empty; } \
         Object.prototype.hasOwnProperty.call(C, 'value') && \
         Object.prototype.hasOwnProperty.call(C, 'empty') && \
         !Object.prototype.hasOwnProperty.call(new C(), 'value')",
    );
    assert_eq!(result, Ok(Value::Boolean(true)));
}

#[test]
fn test_class_static_generator_method_is_callable() {
    let mut ctx = Context::new().unwrap();
    let result = ctx.eval(
        "class C { static *method() { yield 42; } } \
         C.method().next().value",
    );
    assert_eq!(result, Ok(Value::Number(42.0)));
}

#[test]
fn test_static_async_generator_before_computed_instance_fields() {
    let mut ctx = Context::new().unwrap();
    let result = ctx.eval(
        "var key = 'b'; class C { static async *method() { return 42; } [key] = 42; } \
         C.method().next().value",
    );
    assert!(
        result.is_ok(),
        "static async generator failed: {:?}",
        result
    );
}

#[test]
fn test_object_keys_excludes_enumerable_symbol_properties() {
    let mut ctx = Context::new().unwrap();
    let result = ctx.eval(
        "var key = Symbol(); var value = {}; value[key] = 1; \
         value.a = 2; Object.keys(value).length",
    );
    assert_eq!(result, Ok(Value::Number(1.0)));
}

#[test]
fn test_arrow_function_name_is_an_own_property() {
    let mut ctx = Context::new().unwrap();
    let result = ctx.eval(
        "var descriptor = Object.getOwnPropertyDescriptor(() => {}, 'name'); \
         descriptor.value === '' && !descriptor.writable && \
         !descriptor.enumerable && descriptor.configurable",
    );
    assert_eq!(result, Ok(Value::Boolean(true)));
}

#[test]
fn test_computed_symbol_class_field_descriptor_preserves_value() {
    let mut ctx = Context::new().unwrap();
    let result = ctx.eval(
        "var key = Symbol(); class C { [key] = 42; } \
         var c = new C(); Object.getOwnPropertyDescriptor(c, key).value",
    );
    assert_eq!(result, Ok(Value::Number(42.0)));
}
