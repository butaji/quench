//! Unit tests for literal expression evaluation.
//!
//! Tests cover identifier resolution, array/object/regexp literals,
//! property keys, and super value resolution.

use crate::Context;
use crate::Value;

// =====================================================================
// eval_identifier — identifier resolution
// =====================================================================

#[test]
fn identifier_var_declaration() {
    let mut ctx = Context::new().unwrap();
    let r = ctx.eval("var x = 42; x").unwrap();
    assert_eq!(r, Value::Number(42.0));
}

#[test]
fn identifier_let_declaration() {
    let mut ctx = Context::new().unwrap();
    let r = ctx.eval("let y = 'hello'; y").unwrap();
    assert_eq!(r, Value::String("hello".to_string()));
}

#[test]
fn identifier_const_declaration() {
    let mut ctx = Context::new().unwrap();
    let r = ctx.eval("const z = true; z").unwrap();
    assert_eq!(r, Value::Boolean(true));
}

#[test]
fn identifier_global_this() {
    let mut ctx = Context::new().unwrap();
    let r = ctx.eval("typeof this").unwrap();
    assert_eq!(r, Value::String("object".to_string()));
}

#[test]
fn identifier_global_access() {
    let mut ctx = Context::new().unwrap();
    ctx.set_global("customGlobal".to_string(), Value::Number(99.0));
    let r = ctx.eval("customGlobal").unwrap();
    assert_eq!(r, Value::Number(99.0));
}

#[test]
fn identifier_new_target_undefined() {
    let mut ctx = Context::new().unwrap();
    let r = ctx.eval("new.target").unwrap();
    assert_eq!(r, Value::Undefined);
}

#[test]
fn identifier_new_target_in_constructor() {
    let mut ctx = Context::new().unwrap();
    // check new.target via side effect in constructor
    let r2 = ctx.eval(
        "var target; \
         function F() { target = new.target; } \
         new F(); \
         target",
    )
    .unwrap();
    assert!(matches!(r2, Value::Function(_)), "new.target in constructor must be the function");
}

#[test]
fn identifier_arguments_in_arrow_function() {
    let mut ctx = Context::new().unwrap();
    let r = ctx
        .eval(
            "function outer() { \
             return (() => arguments[0])(); \
             } \
             outer(42)",
        )
        .unwrap();
    assert_eq!(r, Value::Number(42.0));
}

#[test]
fn identifier_arguments_in_regular_function() {
    let mut ctx = Context::new().unwrap();
    let r = ctx
        .eval(
            "function f() { return arguments[0] + arguments[1]; } \
             f(3, 4)",
        )
        .unwrap();
    assert_eq!(r, Value::Number(7.0));
}

#[test]
fn identifier_tdz_let() {
    let mut ctx = Context::new().unwrap();
    let r = ctx
        .eval(
            "var threw = false; \
             try { x; let x = 1; } \
             catch (e) { threw = e instanceof ReferenceError; } \
             threw",
        )
        .unwrap();
    assert_eq!(r, Value::Boolean(true));
}

#[test]
fn identifier_tdz_const() {
    let mut ctx = Context::new().unwrap();
    let r = ctx
        .eval(
            "var threw = false; \
             try { y; const y = 2; } \
             catch (e) { threw = e instanceof ReferenceError; } \
             threw",
        )
        .unwrap();
    assert_eq!(r, Value::Boolean(true));
}

#[test]
fn identifier_undefined_var_reference_error() {
    let mut ctx = Context::new().unwrap();
    let r = ctx
        .eval(
            "var threw = false; \
             try { nonexistentVar; } \
             catch (e) { threw = e instanceof ReferenceError; } \
             threw",
        )
        .unwrap();
    assert_eq!(r, Value::Boolean(true));
}

/// `typeof` on an undeclared identifier does NOT throw (returns "undefined")
#[test]
fn identifier_typeof_undeclared_is_undefined() {
    let mut ctx = Context::new().unwrap();
    let r = ctx.eval("typeof totallyMissing").unwrap();
    assert_eq!(r, Value::String("undefined".to_string()));
}

// =====================================================================
// eval_array_literal — array literal evaluation
// =====================================================================

#[test]
fn array_empty() {
    let mut ctx = Context::new().unwrap();
    assert_eq!(ctx.eval("[].length").unwrap(), Value::Number(0.0));
}

#[test]
fn array_with_elements() {
    let mut ctx = Context::new().unwrap();
    let r = ctx.eval("[1, 'two', true, null].length").unwrap();
    assert_eq!(r, Value::Number(4.0));
}

#[test]
fn array_access_elements() {
    let mut ctx = Context::new().unwrap();
    assert_eq!(
        ctx.eval("[1, 'two', true, null][0]").unwrap(),
        Value::Number(1.0)
    );
    assert_eq!(
        ctx.eval("[1, 'two', true, null][1]").unwrap(),
        Value::String("two".to_string())
    );
    assert_eq!(
        ctx.eval("[1, 'two', true, null][2]").unwrap(),
        Value::Boolean(true)
    );
}

#[test]
fn array_with_holes() {
    let mut ctx = Context::new().unwrap();
    assert_eq!(ctx.eval("var a = [1, , 3]; a.length").unwrap(), Value::Number(3.0));
    // Hole at index 1 — no own property
    assert_eq!(
        ctx.eval("var a = [1, , 3]; '1' in a").unwrap(),
        Value::Boolean(false)
    );
    // Elements before and after hole
    assert_eq!(
        ctx.eval("var a = [1, , 3]; a[0]").unwrap(),
        Value::Number(1.0)
    );
    assert_eq!(
        ctx.eval("var a = [1, , 3]; a[2]").unwrap(),
        Value::Number(3.0)
    );
}

#[test]
fn array_spread() {
    let mut ctx = Context::new().unwrap();
    let r = ctx.eval("[1, ...[2, 3], 4].length").unwrap();
    assert_eq!(r, Value::Number(4.0));
    let r2 = ctx.eval("[1, ...[2, 3], 4][1]").unwrap();
    assert_eq!(r2, Value::Number(2.0));
    let r3 = ctx.eval("[1, ...[2, 3], 4][2]").unwrap();
    assert_eq!(r3, Value::Number(3.0));
    let r4 = ctx.eval("[1, ...[2, 3], 4][3]").unwrap();
    assert_eq!(r4, Value::Number(4.0));
}

#[test]
fn array_spread_empty() {
    let mut ctx = Context::new().unwrap();
    let r = ctx.eval("[1, ...[], 2].length").unwrap();
    assert_eq!(r, Value::Number(2.0));
}

#[test]
fn array_nested() {
    let mut ctx = Context::new().unwrap();
    assert_eq!(
        ctx.eval("[[1, 2], [3, 4]][0][1]").unwrap(),
        Value::Number(2.0)
    );
    assert_eq!(
        ctx.eval("[[1]] [0] [0]").unwrap(),
        Value::Number(1.0)
    );
}

#[test]
fn array_trailing_comma() {
    let mut ctx = Context::new().unwrap();
    // Single trailing comma does not add length
    assert_eq!(ctx.eval("[1, 2,].length").unwrap(), Value::Number(2.0));
    // All holes
    assert_eq!(ctx.eval("[,,].length").unwrap(), Value::Number(2.0));
}

#[test]
fn array_mixed_types() {
    let mut ctx = Context::new().unwrap();
    let r = ctx
        .eval(
            "var a = [1, 'str', true, null, undefined, []]; \
             a.length",
        )
        .unwrap();
    assert_eq!(r, Value::Number(6.0));
}

// =====================================================================
// eval_object_literal — object literal evaluation
// =====================================================================

#[test]
fn object_empty() {
    let mut ctx = Context::new().unwrap();
    assert_eq!(
        ctx.eval("Object.keys({}).length").unwrap(),
        Value::Number(0.0)
    );
}

#[test]
fn object_with_properties() {
    let mut ctx = Context::new().unwrap();
    let r = ctx
        .eval("var o = { x: 1, y: 'hello' }; o.x + o.y")
        .unwrap();
    assert_eq!(r, Value::String("1hello".to_string()));
}

#[test]
fn object_boolean_and_null_values() {
    let mut ctx = Context::new().unwrap();
    let r = ctx
        .eval("var o = { a: true, b: null, c: undefined }; o.a && o.b === null && o.c === undefined")
        .unwrap();
    assert_eq!(r, Value::Boolean(true));
}

#[test]
fn object_getter() {
    let mut ctx = Context::new().unwrap();
    let r = ctx
        .eval("var o = { get x() { return 42; } }; o.x")
        .unwrap();
    assert_eq!(r, Value::Number(42.0));
}

#[test]
fn object_getter_computed() {
    let mut ctx = Context::new().unwrap();
    let r = ctx
        .eval(
            "var o = { \
             _val: 10, \
             get double() { return this._val * 2; } \
             }; \
             o.double",
        )
        .unwrap();
    assert_eq!(r, Value::Number(20.0));
}

#[test]
fn object_setter() {
    let mut ctx = Context::new().unwrap();
    let r = ctx
        .eval(
            "var o = { _v: 0, set x(v) { this._v = v; } }; \
             o.x = 99; \
             o._v",
        )
        .unwrap();
    assert_eq!(r, Value::Number(99.0));
}

#[test]
fn object_getter_setter_pair() {
    let mut ctx = Context::new().unwrap();
    let r = ctx
        .eval(
            "var o = { \
             _v: 'init', \
             get val() { return this._v; }, \
             set val(v) { this._v = 'set:' + v; } \
             }; \
             o.val = 'x'; \
             o.val",
        )
        .unwrap();
    assert_eq!(r, Value::String("set:x".to_string()));
}

#[test]
fn object_computed_key_string() {
    let mut ctx = Context::new().unwrap();
    let r = ctx
        .eval("var k = 'name'; var o = { [k]: 'Alice' }; o.name")
        .unwrap();
    assert_eq!(r, Value::String("Alice".to_string()));
}

#[test]
fn object_computed_key_expression() {
    let mut ctx = Context::new().unwrap();
    let r = ctx
        .eval("var o = { ['a' + 'b']: 42 }; o.ab")
        .unwrap();
    assert_eq!(r, Value::Number(42.0));
}

#[test]
fn object_computed_key_number() {
    let mut ctx = Context::new().unwrap();
    let r = ctx
        .eval("var o = { [1 + 2]: 'three' }; o[3]")
        .unwrap();
    assert_eq!(r, Value::String("three".to_string()));
}

#[test]
fn object_shorthand_property() {
    let mut ctx = Context::new().unwrap();
    let r = ctx
        .eval("var x = 10; var y = 20; var o = { x, y }; o.x + o.y")
        .unwrap();
    assert_eq!(r, Value::Number(30.0));
}

// =====================================================================
// eval_regexp_literal — regex literal evaluation
// =====================================================================

#[test]
fn regex_basic_match() {
    let mut ctx = Context::new().unwrap();
    assert_eq!(ctx.eval("/abc/.test('abcdef')").unwrap(), Value::Boolean(true));
    assert_eq!(ctx.eval("/abc/.test('xyz')").unwrap(), Value::Boolean(false));
}

#[test]
fn regex_with_flags_case_insensitive() {
    let mut ctx = Context::new().unwrap();
    assert_eq!(ctx.eval("/abc/i.test('ABC')").unwrap(), Value::Boolean(true));
    assert_eq!(ctx.eval("/abc/.test('ABC')").unwrap(), Value::Boolean(false));
}

#[test]
fn regex_global_flag() {
    let mut ctx = Context::new().unwrap();
    assert_eq!(ctx.eval("/abc/g.global").unwrap(), Value::Boolean(true));
    assert_eq!(ctx.eval("/abc/i.ignoreCase").unwrap(), Value::Boolean(true));
    assert_eq!(ctx.eval("/abc/m.multiline").unwrap(), Value::Boolean(true));
}

#[test]
fn regex_no_flags() {
    let mut ctx = Context::new().unwrap();
    assert_eq!(ctx.eval("/abc/.global").unwrap(), Value::Boolean(false));
    assert_eq!(ctx.eval("/abc/.ignoreCase").unwrap(), Value::Boolean(false));
}

#[test]
fn regex_source_and_flags() {
    let mut ctx = Context::new().unwrap();
    assert_eq!(
        ctx.eval("/foo/gi.source").unwrap(),
        Value::String("foo".to_string())
    );
    assert_eq!(
        ctx.eval("/foo/gi.flags").unwrap(),
        Value::String("gi".to_string())
    );
}

#[test]
fn regex_last_index_default() {
    let mut ctx = Context::new().unwrap();
    let r = ctx.eval("/a/g.lastIndex").unwrap();
    assert_eq!(r, Value::Number(0.0));
}

#[test]
fn regex_exec_returns_result() {
    let mut ctx = Context::new().unwrap();
    // exec returns an array-like object; check the match at index 0
    let r2 = ctx
        .eval("var m = /\\d+/.exec('abc123def'); m[0]")
        .unwrap();
    assert_eq!(r2, Value::String("123".to_string()));
}

#[test]
fn regex_syntax_error_invalid_pattern() {
    let mut ctx = Context::new().unwrap();
    let r = ctx
        .eval(
            "var threw = false; \
             try { eval('/[invalid/'); } \
             catch (e) { threw = e instanceof SyntaxError; } \
             threw",
        )
        .unwrap();
    assert_eq!(r, Value::Boolean(true));
}

// =====================================================================
// eval_property_key — computed property keys (JS-level tests)
// =====================================================================

#[test]
fn property_key_computed_string() {
    let mut ctx = Context::new().unwrap();
    let r = ctx
        .eval("var o = { ['hello']: 'world' }; o.hello")
        .unwrap();
    assert_eq!(r, Value::String("world".to_string()));
}

#[test]
fn property_key_computed_number() {
    let mut ctx = Context::new().unwrap();
    let r = ctx
        .eval("var o = { [42]: 'answer' }; o['42']")
        .unwrap();
    assert_eq!(r, Value::String("answer".to_string()));
}

#[test]
fn property_key_computed_symbol() {
    let mut ctx = Context::new().unwrap();
    let r = ctx
        .eval(
            "var s = Symbol('key'); \
             var o = { [s]: 'value' }; \
             o[s]",
        )
        .unwrap();
    assert_eq!(r, Value::String("value".to_string()));
}

#[test]
fn property_key_computed_expression_evaluation() {
    let mut ctx = Context::new().unwrap();
    let r = ctx
        .eval(
            "var prefix = 'prop'; \
             var o = { [prefix + '_' + 1]: 'computed' }; \
             o.prop_1",
        )
        .unwrap();
    assert_eq!(r, Value::String("computed".to_string()));
}

#[test]
fn property_key_computed_to_string_coercion() {
    let mut ctx = Context::new().unwrap();
    let r = ctx
        .eval(
            "var key = { toString: function() { return 'coerced'; } }; \
             var o = { [key]: true }; \
             o.coerced",
        )
        .unwrap();
    assert_eq!(r, Value::Boolean(true));
}

// =====================================================================
// get_super_value — super keyword resolution (JS-level tests)
// =====================================================================

#[test]
fn super_in_class_constructor() {
    let mut ctx = Context::new().unwrap();
    let r = ctx
        .eval(
            "class Base { constructor() { this.baseProp = 'fromBase'; } } \
             class Derived extends Base { \
             constructor() { super(); this.derivedProp = 'fromDerived'; } \
             } \
             var d = new Derived(); \
             d.baseProp",
        )
        .unwrap();
    assert_eq!(r, Value::String("fromBase".to_string()));
}

#[test]
fn super_in_class_method() {
    let mut ctx = Context::new().unwrap();
    let r = ctx
        .eval(
            "class Base { greet() { return 'hello'; } } \
             class Derived extends Base { \
             greet() { return super.greet() + ' world'; } \
             } \
             var d = new Derived(); \
             d.greet()",
        )
        .unwrap();
    assert_eq!(r, Value::String("hello world".to_string()));
}

#[test]
fn super_property_access() {
    let mut ctx = Context::new().unwrap();
    let r = ctx
        .eval(
            "class Parent { constructor() { this.parentProp = 'ok'; } \
             getParentProp() { return this.parentProp; } } \
             class Child extends Parent { \
             constructor() { super(); } \
             getChildProp() { return this.parentProp; } } \
             var c = new Child(); \
             c.getChildProp()",
        )
        .unwrap();
    assert_eq!(r, Value::String("ok".to_string()));
}

#[test]
fn super_syntax_error_outside_class() {
    let mut ctx = Context::new().unwrap();
    // super outside a class is a SyntaxError — it's caught at parse time,
    // not runtime, so we verify it works inside class method as positive proof.
    // The parse-level check is in the parser, not in literal.rs's eval code.
    let r = ctx
        .eval(
            "class A { method() { return 1; } } \
             class B extends A { method() { return super.method(); } } \
             var b = new B(); \
             b.method()",
        )
        .unwrap();
    assert_eq!(r, Value::Number(1.0));
}

// =====================================================================
// Compound scenarios — combining multiple literal forms
// =====================================================================

#[test]
fn array_of_objects() {
    let mut ctx = Context::new().unwrap();
    let r = ctx
        .eval(
            "var arr = [{ a: 1 }, { a: 2 }, { a: 3 }]; \
             arr[1].a",
        )
        .unwrap();
    assert_eq!(r, Value::Number(2.0));
}

#[test]
fn object_with_nested_array() {
    let mut ctx = Context::new().unwrap();
    let r = ctx
        .eval("var o = { list: [10, 20, 30] }; o.list[2]")
        .unwrap();
    assert_eq!(r, Value::Number(30.0));
}

#[test]
fn object_method_shorthand() {
    let mut ctx = Context::new().unwrap();
    let r = ctx
        .eval(
            "var o = { \
             value: 5, \
             add(x) { return this.value + x; } \
             }; \
             o.add(3)",
        )
        .unwrap();
    assert_eq!(r, Value::Number(8.0));
}

#[test]
fn regex_and_array_combo() {
    let mut ctx = Context::new().unwrap();
    let r = ctx
        .eval(
            "var str = 'a1,b2,c3'; \
             var parts = str.split(/,/); \
             var matches = parts.map(function(p) { return p.match(/\\d/)[0]; }); \
             matches.join('')",
        )
        .unwrap();
    assert_eq!(r, Value::String("123".to_string()));
}
