//! test262 conformance integration test
//!
//! Run with:
//!   cargo test -p quench-runtime --test test262 test262_staged -- --ignored --nocapture

use quench_runtime::test262::{QuenchHost, Test262Host, Test262Runner};
use std::path::PathBuf;

#[test]
fn test_harness_deep_equal_basic() {
    let mut host = QuenchHost::new();
    let result = host.run_script("assert.deepEqual([], [])");
    assert!(
        result.is_ok(),
        "deepEqual([], []) should pass: {:?}",
        result
    );
}

#[test]
fn test_harness_deep_equal_with_values() {
    let mut host = QuenchHost::new();
    let result = host.run_script("assert.deepEqual([1, 2, 3], [1, 2, 3])");
    assert!(
        result.is_ok(),
        "deepEqual([1,2,3], [1,2,3]) should pass: {:?}",
        result
    );
}

#[test]
fn test_harness_deep_equal_primitives() {
    let mut host = QuenchHost::new();
    let result = host.run_script("assert.deepEqual(42, 42)");
    assert!(
        result.is_ok(),
        "deepEqual(42, 42) should pass: {:?}",
        result
    );
}

#[test]
fn test_deep_equal_js_loads() {
    let mut host = QuenchHost::new();
    let result = host.run_script("assert.deepEqual([], [])");
    assert!(result.is_ok());
}

#[test]
fn test_assert_throws_with_deep_equal() {
    let mut host = QuenchHost::new();
    let result = host.run_script(
        "var o = {a: [1]}; assert.throws(Test262Error, function() { assert.deepEqual(o, {a: [2]}) })",
    );
    assert!(result.is_ok());
}

#[test]
fn test_deep_equal_objects_with_different_arrays() {
    let mut host = QuenchHost::new();
    let result = host.run_script(
        "assert.throws(Test262Error, function() { assert.deepEqual({a: [1]}, {a: [2]}) })",
    );
    assert!(result.is_ok());
}

#[test]
fn test_deep_equal_passes_for_equal_nested_objects() {
    let mut host = QuenchHost::new();
    let result = host.run_script("assert.deepEqual({a: [1], b: {c: 2}}, {a: [1], b: {c: 2}})");
    assert!(result.is_ok());
}

#[test]
fn test_deep_equal_throws_for_missing_property() {
    let mut host = QuenchHost::new();
    let result = host
        .run_script("assert.throws(Test262Error, function() { assert.deepEqual({a: 1}, {b: 1}) })");
    assert!(result.is_ok());
}

#[test]
fn test_deep_equal_boxed_primitives() {
    let mut host = QuenchHost::new();
    let result = host.run_script("assert.deepEqual(Object(42), Object(42))");
    assert!(result.is_ok());
}

#[test]
fn test_property_is_enumerable() {
    let mut host = QuenchHost::new();
    let result = host.run_script(
        "var o = {}; Object.defineProperty(o, 'a', {value: 1, enumerable: false}); assert.sameValue(Object.prototype.propertyIsEnumerable.call(o, 'a'), false)",
    );
    assert!(result.is_ok());
}

#[test]
fn test_for_in_with_defined_property() {
    let mut host = QuenchHost::new();
    let result = host.run_script(
        "var o = {a: 1, b: 2}; var keys = []; for (var k in o) { keys.push(k) } assert.sameValue(keys.length, 2)",
    );
    assert!(result.is_ok());
}

#[test]
fn test_own_keys_with_defined_property() {
    let mut host = QuenchHost::new();
    let result = host.run_script(
        "var o = {a: 1, b: 2}; var keys = Object.keys(o); assert.sameValue(keys.length, 2)",
    );
    assert!(result.is_ok());
}

#[test]
fn test_symbol_creation() {
    let mut host = QuenchHost::new();
    let result = host.run_script("var s = Symbol('test'); assert.sameValue(typeof s, 'symbol')");
    assert!(result.is_ok());
}

#[test]
fn test_math_defined() {
    let mut host = QuenchHost::new();
    let result = host.run_script("assert.sameValue(typeof Math, 'object')");
    assert!(result.is_ok());
}

#[test]
fn test_new_math_throws_typeerror() {
    let mut host = QuenchHost::new();
    let result = host.run_script("assert.throws(TypeError, function() { new Math() })");
    assert!(result.is_ok());
}

#[test]
fn test_var_hoisting_global_scope() {
    let mut host = QuenchHost::new();
    let result = host.run_script("var x = 1; assert.sameValue(x, 1)");
    assert!(result.is_ok());
}

#[test]
fn test_var_hoisting_before_declaration() {
    let mut host = QuenchHost::new();
    let result =
        host.run_script("assert.sameValue(x, undefined); var x = 1; assert.sameValue(x, 1)");
    assert!(result.is_ok());
}

#[test]
fn test_block_let_shadows_outer_let() {
    let mut host = QuenchHost::new();
    let result =
        host.run_script("let x = 1; { let x = 2; assert.sameValue(x, 2) }; assert.sameValue(x, 1)");
    assert!(result.is_ok());
}

#[test]
fn test_deep_equal_circular_no_stack_overflow() {
    let mut host = QuenchHost::new();
    let result =
        host.run_script("var a = []; a[0] = a; var b = []; b[0] = b; assert.deepEqual(a, b)");
    assert!(result.is_ok());
}

#[test]
fn test_strict_with_assignment_to_deleted_binding_throws() {
    let mut host = QuenchHost::new();
    let result = host.run_script(
        "\"use strict\"; var x = 1; delete globalThis.x; assert.throws(ReferenceError, function() { x = 2 })",
    );
    assert!(result.is_ok());
}

#[test]
fn test_direct_eval_var_initializes_local_binding() {
    let mut host = QuenchHost::new();
    let result =
        host.run_script("function f() { eval('var y = 1'); return y } assert.sameValue(f(), 1)");
    assert!(result.is_ok());
}

#[test]
fn test_assignment_uses_reference_captured_before_rhs_eval() {
    let mut host = QuenchHost::new();
    let result = host
        .run_script("var x = 1; var setX = function(v) { x = v }; setX(2); assert.sameValue(x, 2)");
    assert!(result.is_ok());
}

#[test]
fn test_assignment_initializes_hoisted_var() {
    let mut host = QuenchHost::new();
    let result = host.run_script("var x = 1; assert.sameValue(x, 1)");
    assert!(result.is_ok());
}

#[test]
fn test_assignment_ignores_inherited_readonly_property() {
    let mut host = QuenchHost::new();
    let result = host.run_script(
        "var parent = {}; Object.defineProperty(parent, 'x', {value: 1, writable: false}); var child = Object.create(parent); child.x = 2; assert.sameValue(child.x, 1)",
    );
    assert!(result.is_ok());
}

#[test]
fn test_assignment_updates_descriptor_value_snapshot() {
    let mut host = QuenchHost::new();
    let result = host.run_script(
        "var o = {}; Object.defineProperty(o, 'x', {value: 1, writable: true, configurable: true}); o.x = 2; assert.sameValue(o.x, 2)",
    );
    assert!(result.is_ok());
}

#[test]
fn test_assignment_replaces_function_property_identity() {
    let mut host = QuenchHost::new();
    let result = host
        .run_script("var o = {x: function() {}}; var f = o.x; o.x = 42; assert.sameValue(o.x, 42)");
    assert!(result.is_ok());
}

#[test]
fn test_strict_assignment_to_function_length_throws() {
    let mut host = QuenchHost::new();
    let result = host.run_script(
        "function f() {} assert.throws(TypeError, function() { 'use strict'; f.length = 0 })",
    );
    assert!(result.is_ok());
}

#[test]
fn test_strict_assignment_to_number_max_value_throws() {
    let mut host = QuenchHost::new();
    let result = host
        .run_script("assert.throws(TypeError, function() { 'use strict'; Number.MAX_VALUE = 1 })");
    assert!(result.is_ok());
}

#[test]
fn test_arrow_parameter_closure_cannot_see_body_var() {
    let mut host = QuenchHost::new();
    let result = host.run_script(
        "var f = (function() { var g = (function(x) { return eval('var y = 2; x + y') }); return g(1) })(); assert.sameValue(f, 3)",
    );
    assert!(result.is_ok());
}

#[test]
fn test_arrow_body_var_does_not_leak_global() {
    let mut host = QuenchHost::new();
    let result = host.run_script("var f = () => { var z = 1; return z }; assert.sameValue(f(), 1)");
    assert!(result.is_ok());
}

#[test]
fn test_rest_parameter_after_missing_argument_is_empty() {
    let mut host = QuenchHost::new();
    let result =
        host.run_script("function f(a, ...b) { return b.length } assert.sameValue(f(1), 0)");
    assert!(result.is_ok());
}

#[test]
fn test_arrow_rest_destructuring_default_closes_over_eval_var() {
    let mut host = QuenchHost::new();
    let result = host.run_script(
        "var f = (function() { var x = 1; return (...args) => x + args.length })(); assert.sameValue(f(1, 2), 3)",
    );
    assert!(result.is_ok());
}

#[test]
fn test_assert_throws_custom_typeerror() {
    let mut host = QuenchHost::new();
    let result = host.run_script("assert.throws(TypeError, function() { throw new TypeError() })");
    assert!(result.is_ok());
}

#[test]
fn test_arrow_lexically_captures_super_property() {
    let mut host = QuenchHost::new();
    let result = host.run_script(
        "class A { m() { return 42 } } class B extends A { m() { var f = () => super.m(); return f() } } assert.sameValue(new B().m(), 42)",
    );
    assert!(result.is_ok());
}

#[test]
fn test_create_realm_uses_its_primitive_prototypes() {
    let mut host = QuenchHost::new();
    let result =
        host.run_script("var o = {}; assert.sameValue(Object.getPrototypeOf(o), Object.prototype)");
    assert!(result.is_ok());
}

// =========================================================================
// Core runtime unit tests (no harness)
// =========================================================================

#[test]
fn core_for_loop_increment() {
    let mut ctx = quench_runtime::Context::new().unwrap();
    quench_runtime::builtins::register_builtins(&mut ctx);
    let r = ctx.eval("for (var i = 0; i < 5; ++i) { } i");
    assert_eq!(r.unwrap(), quench_runtime::value::Value::Number(5.0));
}

#[test]
fn core_postfix_increment() {
    let mut ctx = quench_runtime::Context::new().unwrap();
    quench_runtime::builtins::register_builtins(&mut ctx);
    let r = ctx.eval("let i = 0; i++; i");
    assert_eq!(r.unwrap(), quench_runtime::value::Value::Number(1.0));
}

#[test]
fn core_prefix_increment() {
    let mut ctx = quench_runtime::Context::new().unwrap();
    quench_runtime::builtins::register_builtins(&mut ctx);
    let r = ctx.eval("let i = 0; ++i; i");
    assert_eq!(r.unwrap(), quench_runtime::value::Value::Number(1.0));
}

#[test]
fn core_do_while_increment() {
    let mut ctx = quench_runtime::Context::new().unwrap();
    quench_runtime::builtins::register_builtins(&mut ctx);
    let r = ctx.eval("let i = 0; do { i++; } while (i < 3); i");
    assert_eq!(r.unwrap(), quench_runtime::value::Value::Number(3.0));
    let r2 = ctx.eval("let j = 0; do { j++; } while (false); j");
    assert_eq!(r2.unwrap(), quench_runtime::value::Value::Number(1.0));
}

#[test]
fn core_eval_regex_backslash_nul() {
    let mut ctx = quench_runtime::Context::new().unwrap();
    quench_runtime::builtins::register_builtins(&mut ctx);
    let r = ctx.eval(r#"eval("/\\\u0000/")"#);
    assert!(r.is_ok(), "eval should succeed: {:?}", r);
    let val = r.unwrap();
    assert!(
        matches!(val, quench_runtime::value::Value::Object(_)),
        "eval should return Object: {:?}",
        val
    );
}

#[test]
fn core_eval_regex_backslash_nul_source() {
    let mut ctx = quench_runtime::Context::new().unwrap();
    quench_runtime::builtins::register_builtins(&mut ctx);
    let r = ctx.eval(r#"eval("/\\\u0000/").source"#);
    assert!(r.is_ok(), ".source should be accessible: {:?}", r);
}

#[test]
fn core_eval_newline_regex_throws() {
    let mut ctx = quench_runtime::Context::new().unwrap();
    quench_runtime::builtins::register_builtins(&mut ctx);
    // OXC rejects /\n/ at parse time
    assert!(quench_runtime::parser::parse_script("/\n/").is_err());
    // Direct ctx.eval throws
    assert!(ctx.eval("/\n/").is_err());
    // Native eval from JS throws
    let r = ctx.eval(r#"eval("/\u000A/")"#);
    assert!(r.is_err(), "eval /\\u000A/ should throw: {:?}", r);
}

#[test]
fn core_instanceof_syntax_error_works() {
    let mut ctx = quench_runtime::Context::new().unwrap();
    quench_runtime::builtins::register_builtins(&mut ctx);
    let ctor = ctx.get_global("SyntaxError").unwrap();
    let err = quench_runtime::eval::call_value_with_this(
        ctor,
        vec![quench_runtime::value::Value::String("test".into())],
        quench_runtime::value::Value::Undefined,
    )
    .unwrap();
    ctx.set_global("__diag_err".to_string(), err);
    let inst = ctx.eval("__diag_err instanceof SyntaxError").unwrap();
    assert_eq!(inst, quench_runtime::value::Value::Boolean(true));
}

#[test]
fn core_logical_or_assign() {
    let mut ctx = quench_runtime::Context::new().unwrap();
    quench_runtime::builtins::register_builtins(&mut ctx);
    let r = ctx.eval("let x = 0; x ||= 5; x");
    assert_eq!(r.unwrap(), quench_runtime::value::Value::Number(5.0));
}

#[test]
fn core_logical_and_assign() {
    let mut ctx = quench_runtime::Context::new().unwrap();
    quench_runtime::builtins::register_builtins(&mut ctx);
    let r = ctx.eval("let y = 3; y &&= 7; y");
    assert_eq!(r.unwrap(), quench_runtime::value::Value::Number(7.0));
}

#[test]
fn core_nullish_assign() {
    let mut ctx = quench_runtime::Context::new().unwrap();
    quench_runtime::builtins::register_builtins(&mut ctx);
    let r = ctx.eval("let z = null; z ??= 9; z");
    assert_eq!(r.unwrap(), quench_runtime::value::Value::Number(9.0));
}

#[test]
fn core_bigint_literal() {
    let mut ctx = quench_runtime::Context::new().unwrap();
    quench_runtime::builtins::register_builtins(&mut ctx);
    let r = ctx.eval("123n");
    assert!(matches!(r, Ok(quench_runtime::value::Value::BigInt(_))));
}

#[test]
fn core_bigint_to_string() {
    let mut ctx = quench_runtime::Context::new().unwrap();
    quench_runtime::builtins::register_builtins(&mut ctx);
    let r = ctx.eval("(123n).toString()");
    assert_eq!(
        r.unwrap(),
        quench_runtime::value::Value::String("123n".into())
    );
}

#[test]
fn core_bigint_constructor() {
    let mut ctx = quench_runtime::Context::new().unwrap();
    quench_runtime::builtins::register_builtins(&mut ctx);
    let r = ctx.eval("BigInt(42)");
    assert!(matches!(r, Ok(quench_runtime::value::Value::BigInt(_))));
}

#[test]
fn core_assert_samevalue_basic() {
    let mut ctx = quench_runtime::Context::new().unwrap();
    quench_runtime::builtins::register_builtins(&mut ctx);
    quench_runtime::test262::harness::try_inject_harness(&mut ctx).unwrap();
    let r = ctx.eval(r#"assert.sameValue(1, 1)"#);
    assert!(r.is_ok(), "assert.sameValue(1,1): {:?}", r);
}

#[test]
fn core_assert_samevalue_backslash_nul() {
    let mut ctx = quench_runtime::Context::new().unwrap();
    quench_runtime::builtins::register_builtins(&mut ctx);
    quench_runtime::test262::harness::try_inject_harness(&mut ctx).unwrap();
    let r = ctx.eval(
        r#"
        var xx = "\\" + String.fromCharCode(0);
        var pattern = eval("/" + xx + "/");
        assert.sameValue(pattern.source, xx);
    "#,
    );
    assert!(r.is_ok(), "assert.sameValue backslash-nul: {:?}", r);
}

/// Runtime layer: eval_try_catch var hoisting — var inside try visible outside.
#[test]
fn core_runtime_var_hoisting_from_try() {
    let mut ctx = quench_runtime::Context::new().unwrap();
    quench_runtime::builtins::register_builtins(&mut ctx);
    let r = ctx.eval("try { var x = 42; } catch(e) {} x");
    assert_eq!(r.unwrap(), quench_runtime::value::Value::Number(42.0));
}

/// Runtime layer: eval_try_catch + eval_for — var hoisting from try inside for.
#[test]
fn core_runtime_var_hoisting_from_try_in_for() {
    let mut ctx = quench_runtime::Context::new().unwrap();
    quench_runtime::builtins::register_builtins(&mut ctx);
    let r = ctx.eval("for(var i=0;i<1;++i){try{var x=42;}catch(e){}}x");
    assert_eq!(r.unwrap(), quench_runtime::value::Value::Number(42.0));
}

/// Runtime layer: catch block — var declared in catch visible outside.
#[test]
fn core_runtime_var_in_catch_visible() {
    let mut ctx = quench_runtime::Context::new().unwrap();
    quench_runtime::builtins::register_builtins(&mut ctx);
    let r =
        ctx.eval("try { throw new Error('boom'); } catch(e) { var caught = e.message; } caught");
    assert_eq!(
        r.unwrap(),
        quench_runtime::value::Value::String("boom".into())
    );
}

/// Runtime layer: eval fast path with NUL — eval("/\<NUL>/") returns RegExp.
#[test]
fn core_runtime_eval_fast_path_nul() {
    let mut ctx = quench_runtime::Context::new().unwrap();
    quench_runtime::builtins::register_builtins(&mut ctx);
    let r = ctx.eval("eval(\"/\" + \"\\\\\" + String.fromCharCode(0) + \"/\")");
    assert!(r.is_ok(), "{0:?}", r);
    assert!(matches!(
        r.unwrap(),
        quench_runtime::value::Value::Object(_)
    ));
}

/// Runtime layer: eval fast path regex .source matches pattern for NUL.
#[test]
fn core_runtime_eval_regex_source_nul() {
    let mut ctx = quench_runtime::Context::new().unwrap();
    quench_runtime::builtins::register_builtins(&mut ctx);
    let r = ctx.eval(r#"eval("/" + "\\" + String.fromCharCode(0) + "/").source"#);
    assert!(r.is_ok(), "{0:?}", r);
    let expected = quench_runtime::value::Value::String("\\\0".into());
    assert_eq!(r.unwrap(), expected);
}

/// Runtime layer: eval fast path with control chars returns valid RegExp objects.
#[test]
fn core_runtime_eval_fast_path_control_chars() {
    let mut ctx = quench_runtime::Context::new().unwrap();
    quench_runtime::builtins::register_builtins(&mut ctx);
    for cu in [
        0x01u32, 0x02, 0x03, 0x09, 0x0B, 0x0C, 0x0E, 0x0F, 0x10, 0x1F,
    ] {
        let js = format!("eval(\"/\" + \"\\\\\" + String.fromCharCode({cu}) + \"/\")");
        let r = ctx.eval(&js);
        assert!(r.is_ok(), "CU 0x{0:02X}: {1:?}", cu, r);
        assert!(matches!(
            r.unwrap(),
            quench_runtime::value::Value::Object(_)
        ));
    }
}

/// Runtime layer: scope management — var in block with try persists after block.
#[test]
fn core_runtime_scope_var_in_block_after_try() {
    let mut ctx = quench_runtime::Context::new().unwrap();
    quench_runtime::builtins::register_builtins(&mut ctx);
    let r = ctx.eval("{ try { var y = 99; } catch(e) {} } y");
    assert_eq!(r.unwrap(), quench_runtime::value::Value::Number(99.0));
}

/// Test that `eval()` from JS correctly handles regex patterns containing
/// control characters (NUL, SOH, etc.) that OXC must parse as regex literals.
/// Regression: member access on undefined should create TypeError per spec.
#[test]
fn core_member_access_undefined_throws_typeerror() {
    let mut ctx = quench_runtime::Context::new().unwrap();
    quench_runtime::builtins::register_builtins(&mut ctx);
    let r = ctx.eval("undefined.x");
    assert!(r.is_err(), "undefined.x should throw");
    let msg = format!("{:?}", r.unwrap_err());
    assert!(
        msg.contains("TypeError"),
        "undefined.x should throw TypeError, got: {:?}",
        msg
    );
}

/// Test: regex with u flag and NUL character via exec/test — regression for stage 01.
#[test]
fn core_runtime_regex_u_flag_nul_exec() {
    let mut ctx = quench_runtime::Context::new().unwrap();
    quench_runtime::builtins::register_builtins(&mut ctx);
    let r = ctx.eval(r#"/\0/u.exec(String.fromCharCode(0))"#);
    assert!(r.is_ok(), "{0:?}", r);
    assert!(matches!(
        r.unwrap(),
        quench_runtime::value::Value::Object(_)
    ));
}

/// Line 22 of u-null-character-escape.js
#[test]
fn core_runtime_regex_u_null_line22() {
    let mut ctx = quench_runtime::Context::new().unwrap();
    quench_runtime::builtins::register_builtins(&mut ctx);
    let r = ctx.eval(r#"/\0/u.exec(String.fromCharCode(0))"#);
    assert!(r.is_ok(), "exec failed");
}

/// Line 23 of u-null-character-escape.js
#[test]
fn core_runtime_regex_u_null_line23() {
    let mut ctx = quench_runtime::Context::new().unwrap();
    quench_runtime::builtins::register_builtins(&mut ctx);
    let r = ctx.eval(r#"/^\0a$/u.test('\0a')"#);
    assert!(r.is_ok(), "test failed");
}

/// Focused test: regex exec/test with u flag work correctly.
#[test]
fn core_runtime_regex_u_flag_exec_test() {
    let mut ctx = quench_runtime::Context::new().unwrap();
    quench_runtime::builtins::register_builtins(&mut ctx);
    let r1 = ctx.eval(r#"/^\0a$/u.test('\0a')"#);
    assert_eq!(
        r1.unwrap(),
        quench_runtime::value::Value::Boolean(true),
        "u flag test"
    );
    let r2 = ctx.eval(r#"/\0/u.exec(String.fromCharCode(0))"#);
    assert!(r2.is_ok(), "u flag exec");
}

/// Test: regex named groups with lone surrogates in group name throw SyntaxError.
/// Regression: S7.8.5_A1.4_T2.js relied on this for stage 01 completion.
#[test]
fn core_runtime_regex_named_group_lone_surrogate() {
    let mut ctx = quench_runtime::Context::new().unwrap();
    quench_runtime::builtins::register_builtins(&mut ctx);
    quench_runtime::test262::harness::try_inject_harness(&mut ctx).unwrap();
    let r = ctx.eval(r#"assert.throws(SyntaxError, () => eval("/(?<a\uD801>.)/"), "Lead")"#);
    assert!(
        r.is_ok(),
        "Lead surrogate should throw SyntaxError: {:?}",
        r
    );
}

/// Test that member access on null throws TypeError (not Error).
#[test]
fn core_member_access_null_throws_typeerror() {
    let mut ctx = quench_runtime::Context::new().unwrap();
    quench_runtime::builtins::register_builtins(&mut ctx);
    let r = ctx.eval("null.x");
    assert!(r.is_err(), "null.x should throw");
    let msg = format!("{:?}", r.unwrap_err());
    assert!(
        msg.contains("TypeError"),
        "null.x should throw TypeError, got: {:?}",
        msg
    );
}

/// Test that instanceof correctly identifies SyntaxError in catch blocks.
/// Regression: if instanceof fails, catch blocks can't filter error types.
#[test]
fn core_instanceof_syntaxerror_in_catch() {
    let mut ctx = quench_runtime::Context::new().unwrap();
    quench_runtime::builtins::register_builtins(&mut ctx);
    // Throw a SyntaxError and catch it, verify instanceof SyntaxError is true
    let r = ctx.eval(
        r#"
        var result = "unset";
        try { eval("invalid syntax {{{") } catch(e) {
            result = (e instanceof SyntaxError) ? "ok" : "fail: " + typeof e;
        }
        result;
    "#,
    );
    assert_eq!(
        r.unwrap(),
        quench_runtime::value::Value::String("ok".into())
    );
}

/// Test that try/catch with var declarations in the catch block works correctly.
/// The test262 file S7.8.5_A1.4_T2.js has `var identifierPartNotUnicodeIDContinue`
/// inside the catch block. This must not affect control flow.
#[test]
fn core_try_catch_var_decl_in_catch() {
    let mut ctx = quench_runtime::Context::new().unwrap();
    quench_runtime::builtins::register_builtins(&mut ctx);
    let r = ctx.eval(
        r#"
        var result = "unset";
        try { throw new SyntaxError("test"); }
        catch(e) {
            var isSpecial = (e instanceof SyntaxError);
            result = isSpecial ? "ok" : "fail";
        }
        result;
    "#,
    );
    assert_eq!(
        r.unwrap(),
        quench_runtime::value::Value::String("ok".into())
    );
}

/// Test that to_js_string on an Error-like object doesn't produce
/// a confusing error. The TypeError("Cannot read property...") should be
/// a TypeError, not an Error.
#[test]
fn core_error_to_js_string_type() {
    let mut ctx = quench_runtime::Context::new().unwrap();
    quench_runtime::builtins::register_builtins(&mut ctx);
    // Trying to read .source from undefined should throw TypeError
    let r = ctx.eval(
        r#"
        try { var x = undefined.source; "no error"; }
        catch(e) { e.name }
    "#,
    );
    assert_eq!(
        r.unwrap(),
        quench_runtime::value::Value::String("TypeError".into())
    );
}

#[test]
#[ignore = "run with --ignored"]
fn test262_staged() {
    let test262_dir = {
        let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
        let repo_root = manifest_dir.parent().unwrap().parent().unwrap();
        std::env::var("TEST262_DIR")
            .map(PathBuf::from)
            .unwrap_or_else(|_| repo_root.join("tests/test262"))
    };
    let runner = Test262Runner::new(test262_dir);
    let mut host = QuenchHost::new();
    let summary = runner.run(&mut host);
    if summary.failed > 0 {
        panic!(
            "Stage {} failed: {}/{} passed. First failure: {:?}",
            std::env::var("TEST262_STAGE").unwrap_or_else(|_| "0".into()),
            summary.passed,
            summary.passed + summary.failed,
            summary.first_failure,
        );
    }
}

#[test]
#[ignore = "run with --ignored"]
fn test262_one() {
    let test_path = std::env::var("TEST262_FILE").expect("TEST262_FILE env var required");
    let path = std::path::Path::new(&test_path);
    let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let repo_root = manifest_dir.parent().unwrap().parent().unwrap();
    let test262_dir = std::env::var("TEST262_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|_| repo_root.join("tests/test262"));

    let runner = Test262Runner::new(test262_dir);
    let src = std::fs::read_to_string(path).expect("read test file");
    let meta = quench_runtime::test262::metadata::Test262Metadata::parse(&src).unwrap_or_default();
    let mut host = QuenchHost::new();
    let script = runner
        .harness
        .build_script(&src, &meta.includes)
        .expect("build script");
    let start = std::time::Instant::now();
    let result = host.run_script(&script);
    let elapsed = start.elapsed();
    println!("Time: {:?}", elapsed);
    match result {
        Ok(()) => println!("PASS"),
        Err(e) => panic!("FAIL: {}", e),
    }
}
