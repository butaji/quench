//! Array literal tests

#![allow(clippy::too_many_lines, clippy::complexity)]

#[cfg(test)]
use crate::{Context, Value};

#[cfg(test)]
#[test]
fn test_array_elision_length() {
    // [, ] should produce an array of length 1 with a single hole.
    let mut ctx = Context::new().unwrap();
    let result = ctx.eval("var a = [,]; a.length");
    assert!(result.is_ok(), "array elision failed: {:?}", result);
    assert_eq!(result.unwrap(), Value::Number(1.0));
}

#[cfg(test)]
#[test]
fn test_array_elision_hole_value() {
    // Reading the hole yields undefined.
    let mut ctx = Context::new().unwrap();
    let result = ctx.eval("var a = [,]; a[0]");
    assert!(result.is_ok(), "array elision read failed: {:?}", result);
    assert_eq!(result.unwrap(), Value::Undefined);
}

#[cfg(test)]
#[test]
fn test_array_elision_no_own_property() {
    // An elision contributes to length but not to hasOwnProperty('0').
    let mut ctx = Context::new().unwrap();
    let result = ctx.eval("var a = [,]; a.hasOwnProperty('0')");
    assert!(result.is_ok(), "hasOwnProperty failed: {:?}", result);
    assert_eq!(result.unwrap(), Value::Boolean(false));
}

#[cfg(test)]
#[test]
fn test_array_multiple_elisions() {
    // [,,] produces length 2 with two holes.
    let mut ctx = Context::new().unwrap();
    let result = ctx.eval("var a = [,,]; a.length");
    assert!(result.is_ok(), "array elisions failed: {:?}", result);
    assert_eq!(result.unwrap(), Value::Number(2.0));
}

#[cfg(test)]
#[test]
fn test_array_elision_with_value() {
    // [1, , 3] has length 3 with a hole at index 1.
    let mut ctx = Context::new().unwrap();
    let result = ctx.eval("var a = [1, , 3]; a.length + ',' + a[0] + ',' + a[2]");
    assert!(result.is_ok(), "mixed elision failed: {:?}", result);
    assert_eq!(result.unwrap(), Value::String("3,1,3".to_string()));
}