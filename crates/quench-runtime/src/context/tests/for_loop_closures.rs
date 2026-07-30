//! Tests for `let` closure capture in for-loop per ES §14.7.4.8.
//!
//! These are regression tests for the per-iteration binding semantics:
//! - Test 1: init closure sees initial value (PI captures HEAD, not updated by ++i)
//! - Test 2: condition closure sees 0,1,2,3,4 (fresh PI cell per iteration)
//! - Test 3: update closure sees 1,2,3,4,5 (value after ++i ran in that iteration)

#![allow(clippy::too_many_lines, clippy::complexity)]

#[cfg(test)]
use crate::{Context, Value};

#[cfg(test)]
#[test]
fn let_for_simple_no_closure() {
    let mut ctx = Context::new().unwrap();
    // No closure — just test basic loop termination
    let result = ctx.eval("for (let i = 0; i < 3; ++i) { } 'done';");
    let v = result.unwrap();
    assert_eq!(v, Value::String("done".into()));
}

#[cfg(test)]
#[test]
fn let_for_trivial() {
    let mut ctx = Context::new().unwrap();
    // Trivial: should terminate in 1 iteration
    let result = ctx.eval("for (let i = 0; i < 1; ++i) { } 'done';");
    let v = result.unwrap();
    assert_eq!(v, Value::String("done".into()));
}

#[cfg(test)]
#[test]
fn let_for_trivial_assign() {
    let mut ctx = Context::new().unwrap();
    // With assignment instead of ++
    let result = ctx.eval("for (let i = 0; i < 1; i = i + 1) { } 'done';");
    let v = result.unwrap();
    eprintln!("let_for_trivial_assign: {:?}", v);
    assert_eq!(v, Value::String("done".into()));
}

#[cfg(test)]
#[test]
fn let_for_comparison_works() {
    let mut ctx = Context::new().unwrap();
    // Check that comparison and increment work in sequence
    let result =
        ctx.eval("var r = []; for (let i = 0; i < 2; i = i + 1) { r.push(i); } JSON.stringify(r);");
    let v = result.unwrap();
    eprintln!("let_for_comparison_works: {:?}", v);
    assert_eq!(v, Value::String("[0,1]".into()));
}

#[cfg(test)]
#[test]
fn let_for_with_var() {
    let mut ctx = Context::new().unwrap();
    // var in for-loop (no per-iteration scope) — should work
    let result =
        ctx.eval("var r = []; for (var i = 0; i < 2; i = i + 1) { r.push(i); } JSON.stringify(r);");
    let v = result.unwrap();
    eprintln!("let_for_with_var: {:?}", v);
    assert_eq!(v, Value::String("[0,1]".into()));
}

/// ES §14.7.4.8 — init closure captures the initial value (ES test262
/// `let-closure-inside-initialization.js`).
/// `f` is created once in init (head scope); `++i` updates PI scope, not head.
/// All `f` captures see initial value 0.
#[cfg(test)]
#[test]
fn let_for_init_closure() {
    let mut ctx = Context::new().unwrap();
    let result = ctx.eval(
        r#"
        var a = [];
        for (let i = 0, f = function() { return i; }; i < 5; ++i) {
          a.push(f);
        }
        a[0]();
        "#,
    );
    let v = result.unwrap();
    assert_eq!(
        v,
        Value::Number(5.0),
        "init closure must see initial value (0)"
    );
}

/// ES §14.7.4.8 — condition closure sees per-iteration binding (ES test262
/// `let-closure-inside-condition.js`).
/// Each iteration pushes a closure before checking the condition.
/// There are 6 condition checks (i=0,1,2,3,4,5), the last one is false.
/// Expected: a[0]()=0, a[1]()=1, ..., a[5]()=5 (6 closures, i=0..5).
#[cfg(test)]
#[test]
fn let_for_condition_closure() {
    let mut ctx = Context::new().unwrap();
    let result = ctx.eval(
        r#"
        var a = [];
        for (let i = 0;
             a.push(function() { return i; }), i < 5;
             ++i) { }
        JSON.stringify(a.map(function(f) { return f(); }));
        "#,
    );
    let v = result.unwrap();
    assert_eq!(
        v,
        Value::String("[0,1,2,3,4,5]".into()),
        "condition closures must see per-iteration values: 0,1,2,3,4,5"
    );
}

/// Diagnostic: print each closure's captured value to understand what goes wrong.
#[cfg(test)]
#[test]
fn let_for_condition_closure_diagnostic() {
    let mut ctx = Context::new().unwrap();
    let result = ctx.eval(
        r#"
        var a = [];
        for (let i = 0;
             a.push(function() { return i; }), i < 5;
             ++i) { }
        // Call each closure individually to see what it captured
        var r = [];
        for (var k = 0; k < a.length; k++) { r.push(a[k]()); }
        JSON.stringify(r);
        "#,
    );
    let v = result.unwrap();
    eprintln!("DIAGNOSTIC: condition closures captured: {:?}", v);
    assert_eq!(v, Value::String("[0,1,2,3,4,5]".into()));
}

/// Sanity: body closure should see per-iteration value.
#[cfg(test)]
#[test]
fn let_for_body_closure() {
    let mut ctx = Context::new().unwrap();
    let result = ctx.eval(
        r#"
        var a = [];
        for (let i = 0; i < 3; ++i) {
            a.push(function() { return i; });
        }
        JSON.stringify(a.map(function(f) { return f(); }));
        "#,
    );
    let v = result.unwrap();
    eprintln!("DIAGNOSTIC: body closures captured: {:?}", v);
    assert_eq!(
        v,
        Value::String("[0,1,2]".into()),
        "body closures must see per-iteration values"
    );
}

/// ES §14.7.4.8 — update closure sees the value AFTER `++i` (ES test262
/// `let-closure-inside-next-expression.js`).
/// Closure captures PI cell after `++i` ran in that iteration.
#[cfg(test)]
#[test]
fn let_for_update_closure() {
    let mut ctx = Context::new().unwrap();
    let result = ctx.eval(
        r#"
        var a = [];
        for (let i = 0; i < 5;
             a.push(function() { return i; }), ++i) { }
        JSON.stringify(a.map(function(f) { return f(); }));
        "#,
    );
    let v = result.unwrap();
    assert_eq!(
        v,
        Value::String("[1,2,3,4,5]".into()),
        "update closures must see post-++i values: 1,2,3,4,5"
    );
}

/// Sanity: basic for-loop with `++i` terminates (no infinite loop).
#[cfg(test)]
#[test]
fn let_for_loop_terminates() {
    let mut ctx = Context::new().unwrap();
    let result = ctx.eval(
        r#"
        var count = 0;
        for (let i = 0; i < 3; ++i) { count++; }
        count;
        "#,
    );
    let v = result.unwrap();
    assert_eq!(
        v,
        Value::Number(3.0),
        "for loop must run exactly 3 iterations"
    );
}
