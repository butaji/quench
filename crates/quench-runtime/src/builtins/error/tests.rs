//! Unit tests for builtins/error.rs — Error builtins and Error.prototype.toString.

use crate::value::convert::{to_bool, to_js_string};

// ── Error.prototype.toString ───────────────────────────────────────────────────

#[test]
fn test_error_to_string_name_only() {
    let mut ctx = crate::Context::new().unwrap();
    // new Error() with no message
    let result = ctx.eval("new Error().toString()").unwrap();
    assert_eq!(to_js_string(&result), "Error");
}

#[test]
fn test_error_to_string_message_only() {
    let mut ctx = crate::Context::new().unwrap();
    // Error with custom name property, no message
    let result = ctx
        .eval("var e = new Error(); e.name = 'CustomError'; e.toString()")
        .unwrap();
    assert_eq!(to_js_string(&result), "CustomError");
}

#[test]
fn test_error_to_string_name_and_message() {
    let mut ctx = crate::Context::new().unwrap();
    let result = ctx.eval("new Error('boom').toString()").unwrap();
    assert_eq!(to_js_string(&result), "Error: boom");
}

#[test]
fn test_error_to_string_custom_name_and_message() {
    let mut ctx = crate::Context::new().unwrap();
    let result = ctx
        .eval("var e = new Error('msg'); e.name = 'Foo'; e.toString()")
        .unwrap();
    assert_eq!(to_js_string(&result), "Foo: msg");
}

#[test]
fn test_error_to_string_empty_message() {
    let mut ctx = crate::Context::new().unwrap();
    let result = ctx
        .eval("var e = new Error(''); e.name = 'Bar'; e.toString()")
        .unwrap();
    assert_eq!(to_js_string(&result), "Bar");
}

#[test]
fn test_error_to_string_empty_name() {
    let mut ctx = crate::Context::new().unwrap();
    let result = ctx
        .eval("var e = new Error('x'); e.name = ''; e.toString()")
        .unwrap();
    assert_eq!(to_js_string(&result), "Error: x");
}

#[test]
fn test_error_to_string_both_empty() {
    let mut ctx = crate::Context::new().unwrap();
    let result = ctx
        .eval("var e = new Error(); e.name = ''; e.toString()")
        .unwrap();
    assert_eq!(to_js_string(&result), "Error");
}

#[test]
fn test_error_to_string_name_undefined() {
    let mut ctx = crate::Context::new().unwrap();
    let result = ctx
        .eval("var e = new Error('x'); delete e.name; e.toString()")
        .unwrap();
    assert_eq!(to_js_string(&result), "Error: x");
}

#[test]
fn test_error_to_string_non_string_message() {
    let mut ctx = crate::Context::new().unwrap();
    let result = ctx
        .eval("var e = new Error(); e.message = 42; e.toString()")
        .unwrap();
    assert_eq!(to_js_string(&result), "Error: 42");
}

// ── Error constructor ─────────────────────────────────────────────────────────

#[test]
fn test_error_constructor_no_args() {
    let mut ctx = crate::Context::new().unwrap();
    let result = ctx.eval("new Error().message").unwrap();
    assert_eq!(to_js_string(&result), "");
}

#[test]
fn test_error_constructor_with_message() {
    let mut ctx = crate::Context::new().unwrap();
    let result = ctx.eval("new Error('oops').message").unwrap();
    assert_eq!(to_js_string(&result), "oops");
}

#[test]
fn test_error_name_property() {
    let mut ctx = crate::Context::new().unwrap();
    let result = ctx.eval("new Error().name").unwrap();
    assert_eq!(to_js_string(&result), "Error");
}

// ── Error subclasses ───────────────────────────────────────────────────────────

#[test]
fn test_type_error_name() {
    let mut ctx = crate::Context::new().unwrap();
    let result = ctx.eval("new TypeError().name").unwrap();
    assert_eq!(to_js_string(&result), "TypeError");
}

#[test]
fn test_type_error_message() {
    let mut ctx = crate::Context::new().unwrap();
    let result = ctx.eval("new TypeError('bad').message").unwrap();
    assert_eq!(to_js_string(&result), "bad");
}

#[test]
fn test_type_error_to_string() {
    let mut ctx = crate::Context::new().unwrap();
    let result = ctx.eval("new TypeError('bad').toString()").unwrap();
    assert_eq!(to_js_string(&result), "TypeError: bad");
}

#[test]
fn test_range_error_name() {
    let mut ctx = crate::Context::new().unwrap();
    let result = ctx.eval("new RangeError().name").unwrap();
    assert_eq!(to_js_string(&result), "RangeError");
}

#[test]
fn test_syntax_error_name() {
    let mut ctx = crate::Context::new().unwrap();
    let result = ctx.eval("new SyntaxError().name").unwrap();
    assert_eq!(to_js_string(&result), "SyntaxError");
}

#[test]
fn test_reference_error_name() {
    let mut ctx = crate::Context::new().unwrap();
    let result = ctx.eval("new ReferenceError().name").unwrap();
    assert_eq!(to_js_string(&result), "ReferenceError");
}

#[test]
fn test_eval_error_name() {
    let mut ctx = crate::Context::new().unwrap();
    let result = ctx.eval("new EvalError().name").unwrap();
    assert_eq!(to_js_string(&result), "EvalError");
}

#[test]
fn test_uri_error_name() {
    let mut ctx = crate::Context::new().unwrap();
    let result = ctx.eval("new URIError().name").unwrap();
    assert_eq!(to_js_string(&result), "URIError");
}

// ── Error prototype chain ──────────────────────────────────────────────────────

#[test]
fn test_type_error_instanceof_error() {
    let mut ctx = crate::Context::new().unwrap();
    let result = ctx.eval("new TypeError() instanceof Error").unwrap();
    assert!(to_bool(&result));
}

#[test]
fn test_error_instanceof_object() {
    let mut ctx = crate::Context::new().unwrap();
    let result = ctx.eval("new Error() instanceof Object").unwrap();
    assert!(to_bool(&result));
}

#[test]
fn test_error_prototype_constructor_is_error() {
    let mut ctx = crate::Context::new().unwrap();
    let result = ctx.eval("Error.prototype.constructor === Error").unwrap();
    assert!(to_bool(&result));
}

#[test]
fn test_type_error_prototype_constructor_is_type_error() {
    let mut ctx = crate::Context::new().unwrap();
    let result = ctx
        .eval("TypeError.prototype.constructor === TypeError")
        .unwrap();
    assert!(to_bool(&result));
}

#[test]
fn core_error_message_own_property_spec() {
    let mut ctx = crate::Context::new().unwrap();
    // When called with msg, message IS own property
    let r = ctx
        .eval("var e = new Error('test'); Object.prototype.hasOwnProperty.call(e, 'message')")
        .unwrap();
    assert_eq!(r, crate::value::Value::Boolean(true));
    // When called without args, message is NOT own property
    let r2 = ctx
        .eval("var e2 = new Error(); Object.prototype.hasOwnProperty.call(e2, 'message')")
        .unwrap();
    assert_eq!(r2, crate::value::Value::Boolean(false));
}
