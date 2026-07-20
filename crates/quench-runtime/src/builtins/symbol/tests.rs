//! Tests for Symbol built-in

use crate::builtins::symbol::{register_symbol, reset_global_symbol_registry};
use crate::Context;
use crate::Value;

fn create_test_context() -> Context {
    let mut ctx = Context::new().unwrap();
    register_symbol(&mut ctx);
    ctx
}

#[test]
fn test_symbol_for_returns_same_symbol() {
    let mut ctx = create_test_context();
    reset_global_symbol_registry();

    // Symbol.for('foo') should return the same symbol on repeated calls
    let result1 = ctx.eval("Symbol.for('foo')").unwrap();
    let result2 = ctx.eval("Symbol.for('foo')").unwrap();

    // Both should be the same symbol (same string representation)
    assert_eq!(
        result1.to_string(),
        result2.to_string(),
        "Symbol.for('foo') should return the same symbol on repeated calls"
    );
}

#[test]
fn test_symbol_for_different_keys_different_symbols() {
    let mut ctx = create_test_context();
    reset_global_symbol_registry();

    let foo = ctx.eval("Symbol.for('foo')").unwrap();
    let bar = ctx.eval("Symbol.for('bar')").unwrap();

    assert_ne!(
        foo.to_string(),
        bar.to_string(),
        "Symbol.for('foo') and Symbol.for('bar') should return different symbols"
    );
}

#[test]
fn test_symbol_key_for_registered_symbol() {
    let mut ctx = create_test_context();
    reset_global_symbol_registry();

    // First create a registered symbol
    let _ = ctx.eval("Symbol.for('moon')").unwrap();

    // Symbol.keyFor should return the key
    let result = ctx.eval("Symbol.keyFor(Symbol.for('moon'))").unwrap();
    assert_eq!(
        result,
        Value::String("moon".to_string()),
        "Symbol.keyFor should return 'moon' for the registered symbol"
    );
}

#[test]
fn test_symbol_key_for_unregistered_symbol() {
    let mut ctx = create_test_context();
    reset_global_symbol_registry();

    // Symbol() creates unregistered symbols
    let result = ctx.eval("Symbol.keyFor(Symbol('moon'))").unwrap();
    assert_eq!(
        result,
        Value::Undefined,
        "Symbol.keyFor should return undefined for unregistered symbols"
    );
}

#[test]
fn test_symbol_for_empty_string() {
    let mut ctx = create_test_context();
    reset_global_symbol_registry();

    // Empty string is valid
    let result = ctx.eval("typeof Symbol.for('')").unwrap();
    assert_eq!(
        result,
        Value::String("symbol".to_string()),
        "Symbol.for('') should return a symbol"
    );
}

#[test]
fn test_symbol_for_ignores_this_value() {
    let mut ctx = create_test_context();
    reset_global_symbol_registry();

    let foo = ctx.eval("Symbol.for('foo')").unwrap();

    // Symbol.for.call(String, "foo") should still return the same symbol
    let result = ctx.eval("Symbol.for.call(String, 'foo')").unwrap();
    assert_eq!(
        foo.to_string(),
        result.to_string(),
        "Symbol.for.call should ignore the 'this' value"
    );
}

#[test]
fn test_symbol_anonymous_not_in_registry() {
    let mut ctx = create_test_context();
    reset_global_symbol_registry();

    // Symbol('123') should NOT be in the registry
    let _ = ctx.eval("Symbol('123')").unwrap();

    // Symbol.for('123') should return a DIFFERENT symbol (not in registry yet)
    let result = ctx.eval("Symbol.keyFor(Symbol('123'))").unwrap();
    assert_eq!(
        result,
        Value::Undefined,
        "Anonymous Symbol('123') should not be in the registry"
    );
}

#[test]
fn test_symbol_calls_are_unique() {
    let mut ctx = create_test_context();
    reset_global_symbol_registry();

    let result = ctx.eval("Symbol() === Symbol()").unwrap();
    assert_eq!(result, Value::Boolean(false), "Symbol() !== Symbol()");

    let result = ctx.eval("var s = Symbol('a'); s === s").unwrap();
    assert_eq!(result, Value::Boolean(true), "s === s");
}

#[test]
fn test_symbol_for_identity() {
    let mut ctx = create_test_context();
    reset_global_symbol_registry();

    let result = ctx.eval("Symbol.for('a') === Symbol.for('a')").unwrap();
    assert_eq!(
        result,
        Value::Boolean(true),
        "Symbol.for('a') === Symbol.for('a')"
    );

    let result = ctx.eval("Symbol('a') === Symbol.for('a')").unwrap();
    assert_eq!(
        result,
        Value::Boolean(false),
        "Symbol('a') !== Symbol.for('a')"
    );
}
