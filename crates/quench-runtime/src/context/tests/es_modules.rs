//! ES Module tests

#![allow(clippy::too_many_lines, clippy::complexity)]

#[cfg(test)]
use crate::Context;

#[cfg(test)]
#[test]
fn test_es_module_basic() {
    let mut ctx = Context::new().unwrap();
    let result = ctx.eval_es_module(
        r#"
        export const x = 42;
        export function getX() { return x; }
    "#,
    );
    assert!(result.is_ok(), "basic ES module failed: {:?}", result);
}

#[cfg(test)]
#[test]
fn test_es_module_default_export() {
    let mut ctx = Context::new().unwrap();
    let result = ctx.eval_es_module(
        r#"
        export default function() { return 42; }
    "#,
    );
    assert!(result.is_ok(), "default export failed: {:?}", result);
}

#[test]
fn top_level_await_using_initializes_module_binding() {
    let mut ctx = Context::new().unwrap();
    let result = ctx
        .eval_es_module("await using resource = null; resource")
        .unwrap();
    assert_eq!(result, crate::Value::Null);
}

#[test]
fn module_await_using_block_shadow_does_not_clear_outer_binding() {
    let mut ctx = Context::new().unwrap();
    let result = ctx
        .eval_es_module(
            "await using resource = null; { await using resource = undefined; } resource",
        )
        .unwrap();
    assert_eq!(result, crate::Value::Null);
}

#[test]
fn module_await_using_for_binding_shadows_outer_binding() {
    let mut ctx = Context::new().unwrap();
    let result = ctx
        .eval_es_module(
            "await using outer = null; var i = 0; for (await using inner = undefined; i < 1; i++) { outer } outer",
        )
        .unwrap();
    assert_eq!(result, crate::Value::Null);
}
