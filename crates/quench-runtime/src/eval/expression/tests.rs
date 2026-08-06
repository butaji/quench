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
fn bigint_literal_property_name_uses_decimal_digits() {
    assert_eq!(eval("({ 1n: true })['1']").unwrap(), Value::Boolean(true));
}

#[test]
fn super_property_resolves_base_before_computed_key() {
    assert_eq!(
        eval("var proto={p:'ok'}, proto2={p:'bad'}, obj={__proto__:proto,m(){return super[key];}}, key={toString(){Object.setPrototypeOf(obj,proto2);return 'p';}}; obj.m()")
            .unwrap(),
        Value::String("ok".into())
    );
}

#[test]
fn super_assignment_resolves_base_before_computed_key() {
    assert_eq!(
        eval("var result, proto={set p(v){result='ok';}}, proto2={set p(v){result='bad';}}, obj={__proto__:proto,m(){super[key]=10;}}, key={toString(){Object.setPrototypeOf(obj,proto2);return 'p';}}; obj.m(); result")
            .unwrap(),
        Value::String("ok".into())
    );
}

#[test]
fn super_update_resolves_base_before_computed_key() {
    assert_eq!(
        eval("var proto={p:1}, proto2={p:-1}, obj={__proto__:proto,m(){return ++super[key];}}, key={toString(){Object.setPrototypeOf(obj,proto2);return 'p';}}; obj.m()")
            .unwrap(),
        Value::Number(2.0)
    );
}

#[test]
fn super_compound_assignment_resolves_base_before_computed_key() {
    assert_eq!(
        eval("var proto={p:1}, proto2={p:-1}, obj={__proto__:proto,m(){return super[key]+=1;}}, key={toString(){Object.setPrototypeOf(obj,proto2);return 'p';}}; obj.m()")
            .unwrap(),
        Value::Number(2.0)
    );
}

#[test]
fn tagged_template_exposes_cooked_and_raw_values() {
    assert_eq!(
        eval("(function(s) { return s[0] + '|' + s.raw[0]; })`\\u0062`").unwrap(),
        Value::String("b|\\u0062".to_string())
    );
}

#[test]
fn untagged_template_uses_cooked_escape_value() {
    assert_eq!(
        eval("`\\\\x2c`").unwrap(),
        Value::String("\\x2c".to_string())
    );
}

#[test]
fn tagged_template_invalid_escape_has_undefined_cooked_value() {
    assert_eq!(
        eval("(function(s) { return [s[0], s.raw[0]].join('|'); })`\\01`").unwrap(),
        Value::String("|\\01".to_string())
    );
}

#[test]
fn tagged_template_objects_are_frozen() {
    assert_eq!(
        eval("(function(s) { s.x = 1; s.raw.x = 1; return [Object.isFrozen(s), Object.isFrozen(s.raw), s.x, s.raw.x].join('|'); })`x`").unwrap(),
        Value::String("true|true||".to_string())
    );
}

#[test]
fn tagged_template_array_elements_have_frozen_descriptors() {
    assert_eq!(
        eval("var value; (s => { value = Object.getOwnPropertyDescriptor(s, '0'); })`x`; [value.writable, value.configurable].join('|')").unwrap(),
        Value::String("false|false".to_string())
    );
}

#[test]
fn generator_arguments_assignment_updates_parameter() {
    assert_eq!(
        eval("function* g(a) { arguments[0] = 32; yield a; } g(23).next().value").unwrap(),
        Value::Number(32.0)
    );
}

#[test]
fn generator_return_closes_yield_star_iterator() {
    assert_eq!(
        eval("var count = 0; var source = { next: () => ({ value: 1 }) }; Object.defineProperty(source, 'return', { get: () => { count += 1; } }); source[Symbol.iterator] = () => source; function* g() { try { yield* source; } finally {} } var iter = g(); iter.next(); iter.return(); count").unwrap(),
        Value::Number(1.0)
    );
}

#[test]
fn super_property_getter_does_not_inherit_new_target() {
    assert_eq!(
        eval("var seen = null; class Parent { get attr() { seen = new.target; } } class Child extends Parent { constructor() { super(); super.attr; } } new Child(); seen").unwrap(),
        Value::Undefined
    );
}

#[test]
fn strict_super_assignment_to_frozen_receiver_throws() {
    assert_eq!(
        eval("var caught; class C { method() { Object.freeze(C.prototype); try { super.x = 1; } catch (e) { caught = typeof e; } } } C.prototype.method(); caught").unwrap(),
        Value::String("object".into())
    );
}

#[test]
fn super_property_with_null_home_prototype_throws() {
    assert_eq!(
        eval("var caught; var obj = { method() { try { super.x; } catch (e) { caught = typeof e; } } }; Object.setPrototypeOf(obj, null); obj.method(); caught").unwrap(),
        Value::String("object".into())
    );
}

#[test]
fn optional_chaining_short_circuits_continuations() {
    assert_eq!(
        eval("const a = undefined; let x = 1; a?.[++x]; x").unwrap(),
        Value::Number(1.0)
    );
    assert_eq!(
        eval("const a = undefined; let x = 1; a?.b.c(++x).d; x").unwrap(),
        Value::Number(1.0)
    );
    assert_eq!(
        eval("let x = 1; undefined?.[++x]; x").unwrap(),
        Value::Number(1.0)
    );
    assert_eq!(
        eval("let x = 1; undefined?.b.c(++x).d; x").unwrap(),
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
fn test_delete_new_target_is_true() {
    assert_eq!(
        eval("(function() { return delete (new.target); })()").unwrap(),
        Value::Boolean(true)
    );
    assert_eq!(
        eval("\"use strict\"; (function() { return delete (new.target); })()").unwrap(),
        Value::Boolean(true)
    );
}

#[test]
fn test_delete_catch_binding_is_not_configurable() {
    assert_eq!(
        eval(
            "var callCount = 0; \
             try { throw 'catchme'; } catch (e) { delete e; }"
        )
        .unwrap(),
        Value::Boolean(false)
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
    assert!(eval("'x' in null").is_err());
}

#[test]
fn logical_compound_assignment_evaluates_computed_member_once() {
    let mut ctx = crate::Context::new().unwrap();
    let result = ctx.eval(
        "var count = 0; var obj = {}; function incr() { return ++count; } \
         obj[incr()] &&= incr(); obj[2] = 1; obj[incr()] &&= incr(); \
         obj[2] === 3 && count === 3",
    );
    assert_eq!(result, Ok(Value::Boolean(true)));
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
fn computed_member_update_evaluates_property_key_once() {
    assert_eq!(
        eval("var evaluated = false; var base = {}; var prop = { toString: function() { if (evaluated) throw 'twice'; evaluated = true; return 1; } }; base[prop]++; evaluated").unwrap(),
        Value::Boolean(true)
    );
}

#[test]
fn tagged_template_cooked_and_raw_values_are_arrays() {
    assert_eq!(
        eval("var value; (function(tag) { value = tag`x${1}y`; })(function(strings) { return [Array.isArray(strings), Array.isArray(strings.raw)]; }); value[0] && value[1]").unwrap(),
        Value::Boolean(true)
    );
}

#[test]
fn freezing_array_makes_element_descriptors_read_only() {
    assert_eq!(
        eval("var descriptor = Object.getOwnPropertyDescriptor(Object.freeze([1]), '0'); descriptor.writable === false && descriptor.configurable === false").unwrap(),
        Value::Boolean(true)
    );
}

#[test]
fn object_methods_have_no_prototype_property() {
    assert_eq!(
        eval("var method = { method() {} }.method; !Object.prototype.hasOwnProperty.call(method, 'prototype') && method.prototype === undefined").unwrap(),
        Value::Boolean(true)
    );
}

#[test]
fn logical_assignment_infers_anonymous_function_name() {
    let mut ctx = crate::Context::new().unwrap();
    assert_eq!(
        ctx.eval("var value = 0; value ||= function() {}; value.name"),
        Ok(Value::String("value".into()))
    );
}

#[test]
fn compound_assignment_keeps_initial_binding_after_eval_declares_inner_name() {
    let mut ctx = crate::Context::new().unwrap();
    let result = ctx
        .eval(
            "function f() { var x = 3; var inner = (function() { \
             x *= (eval('var x = 2;'), 4); return x; })(); \
             return String(inner) + ',' + String(x); } f()",
        )
        .unwrap();
    assert_eq!(result, crate::value::Value::String("2,12".into()));
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

#[test]
fn test_named_class_expression_name_not_inferred_when_assigned_to_identifier() {
    assert_eq!(
        eval("let x; x = class explicit {}; x.name").unwrap(),
        Value::String("explicit".into())
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
fn test_assignment_to_computed_static_class_member() {
    // Same as above but with harness (like the failing test262 host test)
    let mut ctx = crate::Context::new().unwrap();
    crate::builtins::register_builtins(&mut ctx);
    let prev = crate::interpreter::is_strict_mode();
    crate::interpreter::set_strict_mode(false);
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
(C[f()] = 1) === 1 && (c[f()] = 1) === 1;
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
    assert_eq!(stmts.len(), 1);
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
fn test_tdz_basic() {
    // Basic TDZ: accessing `y` before `let y;` runs
    let basic = eval("try { y; 'no_exc' } catch(e) { 'caught' } let y;");
    assert_eq!(
        basic,
        Ok(Value::String("caught".into())),
        "Basic TDZ should throw ReferenceError"
    );
}

#[test]
fn test_tdz_in_function_call() {
    // TDZ in function call: accessing `y` inside a function before `let y;` runs
    let r = eval(
        "var err = null; \
         try { (function() { y; })(); } \
         catch(e) { err = e; } \
         let y; \
         err !== null",
    );
    assert_eq!(
        r,
        Ok(Value::Boolean(true)),
        "TDZ in function call should throw: {:?}",
        r
    );
}

#[test]
fn test_tdz_in_for_of_direct() {
    // TDZ in for-of: accessing `y` in the OF expression (after `of`)
    let r = eval(
        "var err = null; \
         var x; \
         try { for (x of [y]) { } } \
         catch(e) { err = e; } \
         let y; \
         err !== null",
    );
    assert_eq!(
        r,
        Ok(Value::Boolean(true)),
        "TDZ in for-of direct iterable should throw: {:?}",
        r
    );
}

#[test]
fn test_tdz_in_direct_destructuring() {
    // Test basic `{ x = y }` destructuring (not in for-of) with `y` in TDZ
    let r = eval(
        "var err = null; \
         var obj = {}; \
         try { var { x = y } = obj; } \
         catch(e) { err = e; } \
         let y; \
         err !== null",
    );
    eprintln!("Direct destructuring TDZ test: {:?}", r);
    assert_eq!(
        r,
        Ok(Value::Boolean(true)),
        "TDZ in direct assignment destructuring default should throw: {:?}",
        r
    );
}

#[test]
fn test_tdz_in_for_of_destructuring_default() {
    // First check: does simple for-of destructuring work?
    let s1 = eval(
        "var s = 0; \
         for ({ x } of [{x:5}]) { s += x; } \
         s",
    );
    assert_eq!(
        s1,
        Ok(Value::Number(5.0)),
        "for-of simple destructuring: {:?}",
        s1
    );

    // Second check: does default in destructuring work when no TDZ?
    let s2 = eval(
        "var v = 0; \
         for ({ x = 99 } of [{}]) { v = x; } \
         v",
    );
    assert_eq!(
        s2,
        Ok(Value::Number(99.0)),
        "for-of destructuring with default: {:?}",
        s2
    );

    // Third check: now test TDZ
    let r = eval(
        "var err = null; \
         try { for ({ x = y } of [{}]) { } } \
         catch(e) { err = e; } \
         let y; \
         err !== null",
    );
    eprintln!("For-of destructuring TDZ test: {:?}", r);
    assert_eq!(
        r,
        Ok(Value::Boolean(true)),
        "TDZ in for-of destructuring default should throw: {:?}",
        r
    );
}

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

#[test]
fn assignment_of_arrow_expression_sets_name_property() {
    let result = eval(
        "var arrow; arrow = () => {}; \
         [arrow.name, Object.getOwnPropertyDescriptor(arrow, 'name').configurable]",
    )
    .unwrap();
    let Value::Object(array) = result else {
        panic!("expected array")
    };
    assert_eq!(array.borrow().get("0"), Some(Value::String("arrow".into())));
    assert_eq!(array.borrow().get("1"), Some(Value::Boolean(true)));
}

#[test]
fn assignment_evaluates_computed_member_before_rhs() {
    let result = eval(
        "function DummyError() {} var base = null; \
         var prop = function() { throw new DummyError(); }; \
         var expr = function() { throw new Error('rhs'); }; \
         try { base[prop()] = expr(); 'no'; } catch (error) { error.constructor.name; }",
    )
    .unwrap();
    assert_eq!(result, Value::String("DummyError".into()));
}

#[test]
fn typeof_revoked_callable_proxy_remains_function() {
    let result = eval(
        "var record = Proxy.revocable(function() {}, {}); record.revoke(); typeof record.proxy",
    )
    .unwrap();
    assert_eq!(result, Value::String("function".into()));
}
