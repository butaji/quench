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
