//! Array and String at() method tests

#![allow(clippy::too_many_lines, clippy::complexity)]

#[cfg(test)]
use crate::{Context, Value};

// ============================================================================
// Array.prototype.at tests
// ============================================================================

#[cfg(test)]
#[test]
fn test_array_at_basic() {
    let mut ctx = Context::new().unwrap();
    ctx.eval("var arr = [1, 2, 3]").unwrap();
    let result = ctx.eval("arr.at(0)");
    assert!(result.is_ok(), "Array.at failed: {:?}", result);
    assert_eq!(result.unwrap(), Value::Number(1.0));
}

#[cfg(test)]
#[test]
fn test_array_at_negative() {
    let mut ctx = Context::new().unwrap();
    ctx.eval("var arr = [1, 2, 3]").unwrap();
    let result = ctx.eval("arr.at(-1)");
    assert!(result.is_ok(), "Array.at negative failed: {:?}", result);
    assert_eq!(result.unwrap(), Value::Number(3.0));
}

#[cfg(test)]
#[test]
fn test_array_at_out_of_bounds() {
    let mut ctx = Context::new().unwrap();
    ctx.eval("var arr = [1, 2, 3]").unwrap();
    let result = ctx.eval("arr.at(10)");
    assert!(result.is_ok(), "Array.at OOB failed: {:?}", result);
    assert_eq!(result.unwrap(), Value::Undefined);
}

// ============================================================================
// String.prototype.at tests
// ============================================================================

#[cfg(test)]
#[test]
fn test_string_at_basic() {
    let mut ctx = Context::new().unwrap();
    let result = ctx.eval("'hello'.at(0)");
    assert!(result.is_ok(), "String.at failed: {:?}", result);
    assert_eq!(result.unwrap(), Value::String("h".to_string()));
}

#[cfg(test)]
#[test]
fn test_string_at_negative() {
    let mut ctx = Context::new().unwrap();
    let result = ctx.eval("'hello'.at(-1)");
    assert!(result.is_ok(), "String.at negative failed: {:?}", result);
    assert_eq!(result.unwrap(), Value::String("o".to_string()));
}

#[cfg(test)]
#[test]
fn test_string_at_out_of_bounds() {
    let mut ctx = Context::new().unwrap();
    let result = ctx.eval("'hello'.at(10)");
    assert!(result.is_ok(), "String.at OOB failed: {:?}", result);
    assert_eq!(result.unwrap(), Value::Undefined);
}
