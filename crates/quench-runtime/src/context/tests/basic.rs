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
fn test_error_throw_catch() {
    use crate::builtins::register_builtins;
    use crate::test262::harness::inject_harness;
    let mut ctx = Context::new().unwrap();
    register_builtins(&mut ctx);
    inject_harness(&mut ctx);

    // Test that throw/catch preserves error object
    let result = ctx.eval(
        r#"
        var caught;
        try {
            throw new Test262Error("test message");
        } catch (e) {
            caught = e;
        }
        caught !== undefined && caught.message;
    "#,
    );
    assert!(result.is_ok(), "eval failed: {:?}", result);
    if let Ok(v) = result {
        println!("caught.message = {:?}", v);
    }
}

#[cfg(test)]
#[test]
fn test_error_properties_directly() {
    use crate::builtins::register_builtins;
    use crate::test262::harness::inject_harness;
    let mut ctx = Context::new().unwrap();
    register_builtins(&mut ctx);
    inject_harness(&mut ctx);

    // Test that Test262Error has message property
    let result = ctx.eval(
        r#"
        var e = new Test262Error("test");
        e.message;
    "#,
    );
    assert!(result.is_ok(), "eval failed: {:?}", result);
    if let Ok(v) = result {
        println!("e.message directly = {:?}", v);
    }
}

#[cfg(test)]
#[test]
fn test_error_return_value() {
    use crate::builtins::register_builtins;
    use crate::test262::harness::inject_harness;
    let mut ctx = Context::new().unwrap();
    register_builtins(&mut ctx);
    inject_harness(&mut ctx);

    // Test what new Test262Error returns
    let result = ctx.eval(
        r#"
        var e = new Test262Error("test");
        typeof e;
    "#,
    );
    assert!(result.is_ok(), "eval failed: {:?}", result);
    if let Ok(v) = result {
        println!("typeof e = {:?}", v);
    }

    // Test object keys
    let result2 = ctx.eval(
        r#"
        var e = new Test262Error("test");
        Object.keys(e);
    "#,
    );
    if let Ok(v) = result2 {
        println!("Object.keys(e) = {:?}", v);
    }
}

#[cfg(test)]
#[test]
fn test_error_catch_bindings() {
    use crate::builtins::register_builtins;
    use crate::test262::harness::inject_harness;
    let mut ctx = Context::new().unwrap();
    register_builtins(&mut ctx);
    inject_harness(&mut ctx);

    // Test that catch parameter receives the thrown value
    let result = ctx.eval(
        r#"
        var e;
        try {
            throw new Test262Error("test");
        } catch (err) {
            e = err;
        }
        typeof e;
    "#,
    );
    assert!(result.is_ok(), "eval failed: {:?}", result);
    if let Ok(v) = result {
        println!("typeof caught = {:?}", v);
    }
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

#[cfg(test)]
#[test]
fn test_async_test_scenario() {
    use crate::builtins::register_builtins;
    use crate::test262::harness::inject_harness;
    let mut ctx = Context::new().unwrap();
    register_builtins(&mut ctx);
    inject_harness(&mut ctx);

    // Simulate the asyncTest scenario
    let result = ctx.eval(
        r#"
        var doneValues = [];
        var doneCallCount = 0;
        function $DONE(error) {
            doneCallCount++;
            print("DONE #" + doneCallCount + " error=" + error);
            doneValues.push(error instanceof TypeError);
        }
        
        // Test 1: asyncTest(null) - should call $DONE with Test262Error
        try {
            var fn = function() { return null; };
            var r = fn();
            r.then(function() {}, function() {});
            "r.then() succeeded - BUG";
        } catch(e) {
            "Caught: " + e;
        }
    "#,
    );
    assert!(result.is_ok(), "eval failed: {:?}", result);
    println!("Result: {:?}", result);
}
