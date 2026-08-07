//! Default parameters tests

#![allow(clippy::too_many_lines, clippy::complexity)]

#[cfg(test)]
use crate::{Context, Value};

#[cfg(test)]
#[test]
fn test_default_parameters_basic() {
    let mut ctx = Context::new().unwrap();
    let result = ctx.eval(
        r#"
        function f(a = 42) { return a; }
        f();
    "#,
    );
    assert!(result.is_ok(), "default params basic failed: {:?}", result);
    assert_eq!(result.unwrap(), Value::Number(42.0));
}

#[cfg(test)]
#[test]
fn test_default_parameters_explicit_undefined() {
    let mut ctx = Context::new().unwrap();
    let result = ctx.eval(
        r#"
        function f(a = 42) { return a; }
        f(undefined);
    "#,
    );
    assert!(
        result.is_ok(),
        "default params with undefined failed: {:?}",
        result
    );
    assert_eq!(result.unwrap(), Value::Number(42.0));
}

#[cfg(test)]
#[test]
fn test_default_parameters_provided_value() {
    let mut ctx = Context::new().unwrap();
    let result = ctx.eval(
        r#"
        function f(a = 42) { return a; }
        f(100);
    "#,
    );
    assert!(
        result.is_ok(),
        "default params override failed: {:?}",
        result
    );
    assert_eq!(result.unwrap(), Value::Number(100.0));
}

#[cfg(test)]
#[test]
fn test_default_parameters_expression() {
    let mut ctx = Context::new().unwrap();
    let result = ctx.eval(
        r#"
        function f(a = 10 + 20) { return a; }
        f();
    "#,
    );
    assert!(
        result.is_ok(),
        "default params expression failed: {:?}",
        result
    );
    assert_eq!(result.unwrap(), Value::Number(30.0));
}

#[cfg(test)]
#[test]
fn test_default_parameters_multiple() {
    let mut ctx = Context::new().unwrap();
    let result = ctx.eval(
        r#"
        function f(a = 1, b = 2, c = 3) { return a + b + c; }
        f();
    "#,
    );
    assert!(
        result.is_ok(),
        "default params multiple failed: {:?}",
        result
    );
    assert_eq!(result.unwrap(), Value::Number(6.0));
}

#[cfg(test)]
#[test]
fn test_default_parameters_partial() {
    let mut ctx = Context::new().unwrap();
    let result = ctx.eval(
        r#"
        function f(a, b = 2, c = 3) { return a + b + c; }
        f(1);
    "#,
    );
    assert!(
        result.is_ok(),
        "default params partial failed: {:?}",
        result
    );
    assert_eq!(result.unwrap(), Value::Number(6.0));
}

#[cfg(test)]
#[test]
fn test_default_parameters_with_other_values() {
    let mut ctx = Context::new().unwrap();
    let result = ctx.eval(
        r#"
        function f(a = 42) { return a; }
        f(null);
    "#,
    );
    assert!(
        result.is_ok(),
        "default params with null failed: {:?}",
        result
    );
    assert_eq!(result.unwrap(), Value::Null);

    let result = ctx.eval(
        r#"
        function f(a = 42) { return a; }
        f(0);
    "#,
    );
    assert!(result.is_ok(), "default params with 0 failed: {:?}", result);
    assert_eq!(result.unwrap(), Value::Number(0.0));

    let result = ctx.eval(
        r#"
        function f(a = 42) { return a; }
        f(false);
    "#,
    );
    assert!(
        result.is_ok(),
        "default params with false failed: {:?}",
        result
    );
    assert_eq!(result.unwrap(), Value::Boolean(false));
}
