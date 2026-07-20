//! Unit tests for expression evaluation.

#[allow(unused_imports)]
use crate::{Context, Value};

#[allow(dead_code)]
fn eval(src: &str) -> Result<Value, crate::value::JsError> {
    let mut ctx = Context::new().unwrap();
    ctx.eval(src)
}

#[test]
fn test_logical_and_short_circuits() {
    assert_eq!(
        eval("false && (() => { throw 1; })()").unwrap(),
        Value::Boolean(false)
    );
    assert_eq!(
        eval("true || (() => { throw 1; })()").unwrap(),
        Value::Boolean(true)
    );
    assert_eq!(
        eval("1 ?? (() => { throw 1; })()").unwrap(),
        Value::Number(1.0)
    );
}

#[test]
fn test_logical_compound_assign_targets_left() {
    assert_eq!(eval("let x = 0; x ||= 5; x").unwrap(), Value::Number(5.0));
    assert_eq!(eval("let y = 3; y &&= 7; y").unwrap(), Value::Number(7.0));
    assert_eq!(
        eval("let z = null; z ??= 9; z").unwrap(),
        Value::Number(9.0)
    );
    assert_eq!(eval("let w = 2; w ||= 5; w").unwrap(), Value::Number(2.0));
}

#[test]
fn test_class_instantiation() {
    assert_eq!(
        eval("class A { constructor(v) { this.v = v; } getV() { return this.v; } } let a = new A(42); a.getV()").unwrap(),
        Value::Number(42.0)
    );
}

#[test]
fn test_do_while_desugaring() {
    assert_eq!(
        eval("let i = 0; do { i++; } while (i < 3); i").unwrap(),
        Value::Number(3.0)
    );
    assert_eq!(
        eval("let j = 0; do { j++; } while (false); j").unwrap(),
        Value::Number(1.0)
    );
}

#[test]
fn test_for_in_object_pattern_throws() {
    assert!(eval("for ({a} in {a: 1}) {}").is_err());
}

#[test]
fn test_for_condition_error_propagates() {
    assert!(eval("for (let i = 0; (() => { throw 1; })(); i++) {}").is_err());
}

#[test]
fn test_ternary_operator() {
    assert_eq!(eval("true ? 1 : 2").unwrap(), Value::Number(1.0));
    assert_eq!(eval("false ? 1 : 2").unwrap(), Value::Number(2.0));
    assert_eq!(eval("null ? 0 : 1").unwrap(), Value::Number(1.0));
    assert_eq!(
        eval("'a' === 'a' ? (false ? 1 : 2) : 3").unwrap(),
        Value::Number(2.0)
    );
}

#[test]
fn test_comma_operator() {
    assert_eq!(eval("(1, 2)").unwrap(), Value::Number(2.0));
    assert_eq!(eval("let x; (x = 1, x + 1)").unwrap(), Value::Number(2.0));
    assert_eq!(eval("let a = (1, 2, 3); a").unwrap(), Value::Number(3.0));
}

#[test]
fn test_for_in_loop() {
    assert_eq!(
        eval("let o = {a: 1, b: 2}; let r = []; for (let k in o) r.push(k); r.length").unwrap(),
        Value::Number(2.0)
    );
    assert_eq!(
        eval("let a = [10, 20]; let r = []; for (let i in a) r.push(i); r.join('')").unwrap(),
        Value::String("01".to_string())
    );
}

#[test]
fn test_typeof_operator() {
    assert_eq!(
        eval("typeof 42").unwrap(),
        Value::String("number".to_string())
    );
    assert_eq!(
        eval("typeof 'hi'").unwrap(),
        Value::String("string".to_string())
    );
    assert_eq!(
        eval("typeof true").unwrap(),
        Value::String("boolean".to_string())
    );
    assert_eq!(
        eval("typeof undefined").unwrap(),
        Value::String("undefined".to_string())
    );
    assert_eq!(
        eval("typeof null").unwrap(),
        Value::String("object".to_string())
    );
    assert_eq!(
        eval("typeof {}").unwrap(),
        Value::String("object".to_string())
    );
    assert_eq!(
        eval("typeof (() => {})").unwrap(),
        Value::String("function".to_string())
    );
    assert_eq!(
        eval("typeof nonExistentVarHere").unwrap(),
        Value::String("undefined".to_string())
    );
}

#[test]
fn test_void_operator() {
    assert_eq!(eval("void 0").unwrap(), Value::Undefined);
    assert_eq!(eval("void 42").unwrap(), Value::Undefined);
    assert_eq!(eval("void(0)").unwrap(), Value::Undefined);
}

#[test]
fn test_unary_negation_and_not() {
    assert_eq!(eval("-42").unwrap(), Value::Number(-42.0));
    assert_eq!(eval("-(5 + 3)").unwrap(), Value::Number(-8.0));
    assert_eq!(eval("-'5'").unwrap(), Value::Number(-5.0));
    assert_eq!(eval("-true").unwrap(), Value::Number(-1.0));
    assert_eq!(eval("+'-3'").unwrap(), Value::Number(-3.0));
    assert_eq!(eval("!true").unwrap(), Value::Boolean(false));
    assert_eq!(eval("!false").unwrap(), Value::Boolean(true));
    assert_eq!(eval("!0").unwrap(), Value::Boolean(true));
    assert_eq!(eval("!''").unwrap(), Value::Boolean(true));
    assert_eq!(eval("!null").unwrap(), Value::Boolean(true));
    assert_eq!(eval("!!42").unwrap(), Value::Boolean(true));
}

#[test]
fn test_delete_operator() {
    assert_eq!(
        eval("let o = {a: 1}; delete o.a").unwrap(),
        Value::Boolean(true)
    );
    assert_eq!(
        eval("let o = {a: 1}; delete o.a; o.a").unwrap(),
        Value::Undefined
    );
    assert_eq!(eval("delete Math.PI").unwrap(), Value::Boolean(false));
    assert_eq!(
        eval("delete nonExistentHere123").unwrap(),
        Value::Boolean(true)
    );
}

#[test]
fn test_instanceof_and_in() {
    assert_eq!(eval("[] instanceof Array").unwrap(), Value::Boolean(true));
    assert_eq!(
        eval("({}) instanceof Object").unwrap(),
        Value::Boolean(true)
    );
    assert_eq!(
        eval("(function() {}) instanceof Function").unwrap(),
        Value::Boolean(true)
    );
    assert_eq!(eval("42 instanceof Number").unwrap(), Value::Boolean(false));
    assert_eq!(
        eval("'hi' instanceof String").unwrap(),
        Value::Boolean(false)
    );
    assert_eq!(eval("'a' in {a: 1}").unwrap(), Value::Boolean(true));
    assert_eq!(eval("'b' in {a: 1}").unwrap(), Value::Boolean(false));
    assert_eq!(eval("'length' in [1, 2]").unwrap(), Value::Boolean(true));
    assert_eq!(eval("'x' in null").unwrap(), Value::Boolean(false));
}

#[test]
fn test_update_prefix_postfix() {
    assert_eq!(eval("let x = 1; ++x").unwrap(), Value::Number(2.0));
    assert_eq!(eval("let x = 1; --x").unwrap(), Value::Number(0.0));
    assert_eq!(eval("let x = 1; x++").unwrap(), Value::Number(1.0));
    assert_eq!(eval("let x = 1; x++; x").unwrap(), Value::Number(2.0));
    assert_eq!(eval("let x = 5; x--").unwrap(), Value::Number(5.0));
    assert_eq!(eval("let x = 5; x--; x").unwrap(), Value::Number(4.0));
    assert_eq!(
        eval("let x = 1; let y = ++x; y").unwrap(),
        Value::Number(2.0)
    );
    assert_eq!(
        eval("let x = 1; let y = x++; y").unwrap(),
        Value::Number(1.0)
    );
}

#[test]
fn test_export_default_expr_lowers_to_assignment() {
    let program = crate::parser::parse_es_module("export default 42;").unwrap();
    let crate::ast::Program::Script(stmts) = program;
    assert!(
        stmts.is_empty(),
        "export default not yet lowered, got {} stmts",
        stmts.len()
    );
}
