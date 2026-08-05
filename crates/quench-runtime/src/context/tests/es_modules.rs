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
fn module_exported_let_is_in_tdz_before_initialization() {
    let mut ctx = Context::new().unwrap();
    let parsed =
        crate::parser::parse_es_module("typeof test262; export let test262 = 23;").unwrap();
    let crate::ast::Program::Script(statements) = parsed;
    assert!(
        crate::interpreter::collect_let_const_declarations(&statements)
            .iter()
            .any(|(name, _)| name == "test262")
    );
    let result = ctx.eval_es_module("typeof test262; export let test262 = 23;");
    assert!(result.is_err());
}

#[test]
fn module_import_meta_is_an_object() {
    let mut ctx = Context::new().unwrap();
    let result = ctx
        .eval_es_module("typeof import.meta === 'object' && import.meta === import.meta")
        .unwrap();
    assert_eq!(result, crate::Value::Boolean(true));
}

#[test]
fn top_level_for_await_accepts_awaited_array_expression() {
    let mut ctx = Context::new().unwrap();
    let result = ctx.eval_es_module(
        "var binding;\
        for await (binding of [await []]) { await []; break; }\
        for await (var binding of [await []]) { await []; break; }\
        for await (let binding of [await []]) { await []; break; }",
    );
    assert!(result.is_ok(), "for-await module failed: {:?}", result);
}

#[test]
fn module_exported_function_is_initialized_before_module_body() {
    let mut ctx = Context::new().unwrap();
    let result = ctx.eval_es_module("typeof test262; export function test262() {};");
    assert_eq!(result, Ok(crate::Value::String("function".to_string())));
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
