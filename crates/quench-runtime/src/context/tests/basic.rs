//! Basic context tests

#![allow(clippy::too_many_lines, clippy::complexity)]

#[cfg(test)]
use crate::{Context, Value};

#[cfg(test)]
#[test]
fn test_context_creation() {
    let ctx = Context::new();
    assert!(ctx.is_ok());
}

#[cfg(test)]
#[test]
fn test_globals() {
    let mut ctx = Context::new().unwrap();
    ctx.set_global("test".to_string(), Value::Number(42.0));
    assert_eq!(ctx.get_global("test"), Some(Value::Number(42.0)));
}

#[test]
fn global_property_assignment_updates_global_property() {
    let mut ctx = Context::new().unwrap();
    let result = ctx.eval("globalThis.shared = 2; globalThis.shared === 2;");
    assert_eq!(result, Ok(Value::Boolean(true)));
}

#[cfg(test)]
#[test]
fn test_eval_simple() {
    let mut ctx = Context::new().unwrap();
    let result = ctx.eval("1 + 2");
    assert!(result.is_ok());
    if let Ok(v) = result {
        assert_eq!(v, Value::Number(3.0));
    }
}

#[cfg(test)]
#[test]
fn test_console_exists() {
    let ctx = Context::new().unwrap();
    let console = ctx.get_global("console");
    assert!(console.is_some());
}

#[cfg(test)]
#[test]
fn test_global_this_assignment() {
    let mut ctx = Context::new().unwrap();
    let result = ctx.eval("typeof globalThis");
    assert!(result.is_ok(), "typeof globalThis failed: {:?}", result);
    assert_eq!(result.unwrap(), Value::String("object".to_string()));

    let result = ctx.eval("globalThis.testProp = 42");
    assert!(result.is_ok(), "globalThis assignment failed: {:?}", result);

    let result = ctx.eval("globalThis.testProp");
    assert!(result.is_ok(), "globalThis read failed: {:?}", result);
    assert_eq!(result.unwrap(), Value::Number(42.0));
}

#[cfg(test)]
#[test]
fn test_date_prototype_access() {
    let mut ctx = Context::new().unwrap();
    let result = ctx.eval("Date.prototype");
    assert!(result.is_ok(), "Date.prototype failed: {:?}", result);

    let result = ctx.eval("Date.prototype.toLocaleTimeString");
    assert!(
        result.is_ok(),
        "Date.prototype.toLocaleTimeString failed: {:?}",
        result
    );
}

#[cfg(test)]
#[test]
fn test_function_declaration_overrides_existing_global() {
    let mut ctx = Context::new().unwrap();
    ctx.eval("function mountTree() { return 'runtime'; }")
        .unwrap();
    let result = ctx
        .eval(
            r#"
            function mountTree() { return 'user'; }
            mountTree();
        "#,
        )
        .unwrap();
    assert_eq!(result, Value::String("user".to_string()));
}

#[cfg(test)]
#[test]
fn test_duplicate_function_declaration_last_wins() {
    let mut ctx = Context::new().unwrap();
    let result = ctx
        .eval(
            r#"
            function f() { return 1; }
            function f() { return 2; }
            f();
        "#,
        )
        .unwrap();
    assert_eq!(result, Value::Number(2.0));
}

#[cfg(test)]
#[test]
fn test_null_then_throws() {
    let mut ctx = Context::new().unwrap();
    let result = ctx.eval(
        r#"
        var result = null;
        var caught = null;
        try {
            result.then(function() {}, function() {});
        } catch(e) {
            caught = e;
        }
        caught !== null;
    "#,
    );
    assert!(result.is_ok(), "eval failed: {:?}", result);
    if let Ok(v) = result {
        println!("null.then threw: {:?}", v);
        assert_eq!(v, Value::Boolean(true));
    }
}

#[cfg(test)]
#[test]
fn test_null_property_access_throws() {
    let mut ctx = Context::new().unwrap();
    // Test if accessing property on null throws
    let result = ctx.eval(
        r#"
        null.then;
    "#,
    );
    println!("Result of null.then: {:?}", result);
    assert!(result.is_err(), "Should have thrown an error");
}
