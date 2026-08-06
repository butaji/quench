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
fn error_subclass_no_arg_inherits_prototype_message() {
    let mut ctx = crate::Context::new().unwrap();
    let result = ctx
        .eval(
            "class Err extends Error {} \
             Err.prototype.message = 'custom-error'; \
             var err2 = new Err(); \
             [!err2.hasOwnProperty('message'), err2.message]",
        )
        .unwrap();
    let crate::Value::Object(arr) = result else {
        panic!("expected array")
    };
    let elems = arr.borrow().elements.clone();
    assert_eq!(elems[0], crate::Value::Boolean(true));
    assert_eq!(elems[1], crate::Value::String("custom-error".to_string()));
}

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
fn test_error_subclass_instanceof_error() {
    let mut ctx = crate::Context::new().unwrap();
    crate::builtins::register_builtins(&mut ctx);
    let result = ctx
        .eval(
            "class Subclass extends Error {} \
             var sub = new Subclass(); \
             sub instanceof Subclass && sub instanceof Error",
        )
        .unwrap();
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
fn native_error_constructor_properties_are_non_enumerable() {
    let mut ctx = crate::Context::new().unwrap();
    let result = ctx
        .eval("var n = Object.getOwnPropertyDescriptor(TypeError, 'name'); var l = Object.getOwnPropertyDescriptor(TypeError, 'length'); [Object.getOwnPropertyDescriptor(Error.prototype, 'constructor').enumerable, Object.getOwnPropertyDescriptor(TypeError.prototype, 'constructor').enumerable, n.configurable, l.value].join('|')")
        .unwrap();
    assert_eq!(
        result,
        crate::value::Value::String("false|false|true|1".into())
    );
}

#[test]
fn native_error_constructor_cause_is_an_own_property() {
    let mut ctx = crate::Context::new().unwrap();
    let result = ctx
        .eval("var e = new TypeError('message', {cause: 7}); var d = Object.getOwnPropertyDescriptor(e, 'cause'); [e.cause, d.writable, d.enumerable, d.configurable].join('|')")
        .unwrap();
    assert_eq!(
        result,
        crate::value::Value::String("7|true|false|true".into())
    );
}

#[test]
fn aggregate_error_cause_is_an_own_property() {
    let mut ctx = crate::Context::new().unwrap();
    let result = ctx
        .eval("var e = new AggregateError([], 'message', {cause: 7}); var d = Object.getOwnPropertyDescriptor(e, 'cause'); [e.cause, d.writable, d.enumerable, d.configurable, Object.getOwnPropertyDescriptor(AggregateError.prototype, 'constructor').enumerable].join('|')")
        .unwrap();
    assert_eq!(
        result,
        crate::value::Value::String("7|true|false|true|false".into())
    );
}

#[test]
fn aggregate_error_uses_new_target_prototype_and_standard_length() {
    let mut ctx = crate::Context::new().unwrap();
    let result = ctx
        .eval("var C = function() {}; C.prototype = {marker: 1}; var e = Reflect.construct(AggregateError, [[]], C); var d = Object.getOwnPropertyDescriptor(AggregateError, 'length'); [Object.getPrototypeOf(e).marker, d.value, d.writable].join('|')")
        .unwrap();
    assert_eq!(result, crate::value::Value::String("1|2|false".into()));
}

#[test]
fn aggregate_error_uses_custom_new_target_prototype() {
    let mut ctx = crate::Context::new().unwrap();
    let result = ctx
        .eval("var custom = {x: 42}; var newt = new Proxy(function() {}, {get(t, p) { if (p === 'prototype') return custom; return t[p]; }}); var obj = Reflect.construct(AggregateError, [[]], newt); [Object.getPrototypeOf(obj) === custom, obj.x].join('|')")
        .unwrap();
    assert_eq!(result, crate::value::Value::String("true|42".into()));
}

#[test]
fn aggregate_error_materializes_iterable_errors() {
    let mut ctx = crate::Context::new().unwrap();
    let result = ctx
        .eval("var count = 0; var values = []; var input = {[Symbol.iterator]() { return {next() { count += 1; return {done: count === 3, get value() { values.push(count); }}; }}; }}; new AggregateError(input); [count, values.join(',')].join('|')")
        .unwrap();
    assert_eq!(result, crate::value::Value::String("3|1,2".into()));
}

#[test]
fn aggregate_error_propagates_iterable_errors() {
    let mut ctx = crate::Context::new().unwrap();
    let result = ctx.eval("var input = {get [Symbol.iterator]() { throw new Error('iterator'); }}; try { new AggregateError(input); 'no error'; } catch (error) { error.message; }").unwrap();
    assert_eq!(result, crate::value::Value::String("iterator".into()));
}

#[test]
fn aggregate_error_evaluates_message_before_errors() {
    let mut ctx = crate::Context::new().unwrap();
    let result = ctx
        .eval("var sequence = []; var message = {toString() { sequence.push(1); return ''; }}; var errors = {[Symbol.iterator]() { sequence.push(2); return {next() { sequence.push(3); return {done: true}; }}; }}; new AggregateError(errors, message); sequence.join(',')")
        .unwrap();
    assert_eq!(to_js_string(&result), "1,2,3");
}

#[test]
fn aggregate_error_without_errors_throws_for_undefined_iterator() {
    let mut ctx = crate::Context::new().unwrap();
    let result = ctx
        .eval("try { new AggregateError(); 'no error'; } catch (error) { error instanceof TypeError; }")
        .unwrap();
    assert_eq!(result, crate::value::Value::Boolean(true));
}

#[test]
fn suppressed_error_constructor_metadata_is_standard() {
    let mut ctx = crate::Context::new().unwrap();
    let result = ctx
        .eval("var l = Object.getOwnPropertyDescriptor(SuppressedError, 'length'); var c = Object.getOwnPropertyDescriptor(SuppressedError.prototype, 'constructor'); [l.value, l.writable, l.enumerable, l.configurable, c.enumerable].join('|')")
        .unwrap();
    assert_eq!(
        result,
        crate::value::Value::String("3|false|false|true|false".into())
    );
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
