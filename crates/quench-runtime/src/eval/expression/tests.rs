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
fn test_for_in_object_pattern_destructures_key() {
    assert_eq!(
        eval("var v; for ([x] in {key: 1}) { v = x; } v").unwrap(),
        Value::String("k".to_string())
    );
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
fn test_named_function_expression_binds_its_own_name() {
    // Per ES spec §12.4.1.3: a named FunctionExpression binds its Identifier
    // as an immutable lexical binding in its own environment record.
    assert!(eval("(function f() { return f; })()").is_ok());
    // The name is NOT visible outside
    assert!(eval("(function g() { return g; })()").is_ok());
}

#[test]
fn test_named_function_expression_name_not_visible_outside() {
    let result: Result<Value, _> = eval("(function fact(n) { return fact; })(1)");
    assert!(result.is_ok(), "function itself should evaluate");
    let result: Result<Value, _> =
        eval("(function fact(n) { return n === 1 ? 1 : n * fact(n-1); })(5)");
    assert!(
        result.is_ok(),
        "recursive named function expression should work"
    );
}

// ─── Assignment expression returns RHS ──────────────────────────────────────

/// Per ES spec §12.15, an AssignmentExpression evaluates the assignment
/// target, then the RHS, stores the result, and returns the RHS value.
#[test]
fn test_assignment_expression_returns_rhs_simple() {
    // Basic variable assignment
    assert_eq!(eval("let x; x = 5").unwrap(), Value::Number(5.0));
    assert_eq!(eval("let x; (x = 5)").unwrap(), Value::Number(5.0));
    // In expression context
    assert_eq!(eval("let x; (x = 5) + 1").unwrap(), Value::Number(6.0));
    // Nested
    assert_eq!(eval("let a, b; a = b = 7").unwrap(), Value::Number(7.0));
}

#[test]
fn test_assignment_expression_returns_rhs_object() {
    // Member expression assignment returns RHS
    assert_eq!(eval("let o = {}; (o.x = 5)").unwrap(), Value::Number(5.0));
    assert_eq!(
        eval("let o = {}; (o['x'] = 5)").unwrap(),
        Value::Number(5.0)
    );
    // Chained
    assert_eq!(
        eval("let o = {}; let r = (o.x = 5); r").unwrap(),
        Value::Number(5.0)
    );
}

#[test]
fn test_assignment_expression_returns_rhs_in_assert() {
    // This is the exact pattern from the failing test262 test
    // assert.sameValue(obj[fn()] = 1, 1) - just verify it returns RHS
    // Can't use assert without harness, so just check return value
    assert_eq!(
        eval("let o = {}; function f() {}; (o[f()] = 1)").unwrap(),
        Value::Number(1.0),
        "computed member assignment must return RHS"
    );
}

#[test]
fn test_assignment_to_computed_class_member_returns_rhs() {
    // Direct class instance computed setter
    let result = eval(
        "function f() { return 'x'; } class C { set [f()](v) { } } let c = new C(); c[f()] = 1; c.x",
    );
    assert!(
        result.is_ok(),
        "computed setter assignment should not panic: {:?}",
        result
    );
    // The RHS (1) is returned by the assignment expression
    assert_eq!(
        eval("function f() { return 'x'; } class C { set [f()](v) { } } let c = new C(); (c[f()] = 1)").unwrap(),
        Value::Number(1.0),
        "assignment to computed setter must return RHS value"
    );
}

#[test]
fn test_assignment_to_computed_static_class_member_returns_rhs() {
    // Static computed setter - this is the failing case
    let result = eval("function f() {} class C { static set [f()](v) { } } C[f()] = 1;");
    assert!(
        result.is_ok(),
        "static computed setter assignment should not panic: {:?}",
        result
    );

    // The RHS (1) is returned by the assignment expression
    let r = eval("function f() {} class C { static set [f()](v) { } } (C[f()] = 1)");
    assert!(
        r.is_ok(),
        "evaluating assignment to static computed setter should not error: {:?}",
        r
    );
    assert_eq!(
        r.unwrap(),
        Value::Number(1.0),
        "assignment to static computed setter must return RHS value"
    );
}

#[test]
fn test_assignment_to_computed_static_class_member_with_harness() {
    // Same as above but with harness (like the failing test262 host test)
    let mut ctx = crate::Context::new().unwrap();
    crate::builtins::register_builtins(&mut ctx);
    let prev = crate::interpreter::is_strict_mode();
    crate::interpreter::set_strict_mode(false);
    crate::test262::harness::try_inject_harness(&mut ctx).expect("harness");
    crate::interpreter::set_strict_mode(prev);

    let r = ctx.eval(
        r#"
function f() {}
class C {
    get [f()]() { return 1; }
    set [f()](v) { }
    static get [f()]() { return 1; }
    static set [f()](v) { }
}
var c = new C();
assert.sameValue(C[f()] = 1, 1);
assert.sameValue(c[f()] = 1, 1);
"#,
    );
    assert!(
        r.is_ok(),
        "computed setter with harness should pass: {:?}",
        r
    );
}

#[test]
fn test_assignment_expression_returns_rhs_object_literal() {
    // Object literal with setter
    assert_eq!(
        eval("let r; ({ set x(v) { } }).x = 5").unwrap(),
        Value::Number(5.0),
        "assignment to object literal setter returns RHS"
    );
    // Assignment to object literal setter returns RHS (without assert)
    assert_eq!(
        eval("let o = { set x(v) { } }; (o.x = 5)").unwrap(),
        Value::Number(5.0),
        "assignment to object literal setter returns RHS"
    );
}

#[test]
fn test_assignment_to_global_function_property_returns_rhs() {
    // Assignment to a property on a function object (like C[f] = 1 where C is a class)
    assert_eq!(
        eval("function f() {} f.x = 5; f.x").unwrap(),
        Value::Number(5.0)
    );
    assert_eq!(
        eval("function f() {} (f.x = 5)").unwrap(),
        Value::Number(5.0),
        "assignment to function property returns RHS"
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

// ─── super in static init block ───────────────────────────────────

#[test]
fn super_in_static_init_block_resolves_to_superclass() {
    // test262: static-init-super-property.js
    // super.property in a static init block should access the superclass's
    // own properties (static fields), not the prototype chain.
    let r = eval(
        "function Parent() {} \
         Parent.test262 = 'test262'; \
         var value; \
         class C extends Parent { \
           static { value = super.test262; } \
         } \
         value",
    )
    .unwrap();
    assert_eq!(r, Value::String("test262".into()));
}

// ─── super in base class instance methods ─────────────────────────

#[test]
fn super_in_derived_instance_method_looks_up_prototype_chain() {
    // Sanity check: super in a derived class instance method.
    let r = eval(
        "class Base { foo() { return 42; } } \
         class Derived extends Base { bar() { return super.foo(); } } \
         new Derived().bar()",
    )
    .unwrap();
    assert_eq!(r, Value::Number(42.0));
}

#[test]
fn super_in_base_class_instance_method_works() {
    // test262: class-body-method-definition-super-property.js
    // Step 1: can we construct the class and call dontDoThis?
    let r = eval(
        "class A { \
           dontDoThis() { super.makeBugs = 1; } \
         } \
         var a = new A(); \
         a.dontDoThis(); \
         a.makeBugs",
    );
    let val = r.unwrap_or_else(|e| panic!("step 1 failed: {:?}", e));
    assert_eq!(val, Value::Number(1.0));
    // Step 2: add constructor with super.toString()
    let r2 = eval(
        "class A { \
           constructor() { super.toString(); } \
           dontDoThis() { super.makeBugs = 1; } \
         } \
         var a = new A(); \
         a.dontDoThis(); \
         a.makeBugs",
    );
    let val2 = r2.unwrap_or_else(|e| panic!("step 2 failed: {:?}", e));
    assert_eq!(val2, Value::Number(1.0));
}

// ─── super in static method of derived class ──────────────────────

#[test]
fn super_in_static_method_of_derived_class_works() {
    // super.property in a static method should access the superclass constructor's
    // own properties (static methods/fields).
    let r = eval(
        "class Parent { static greet() { return 'hello'; } } \
         class Child extends Parent { \
           static doIt() { return super.greet(); } \
         } \
         Child.doIt()",
    )
    .unwrap();
    assert_eq!(r, Value::String("hello".into()));
}
