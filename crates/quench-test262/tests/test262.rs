//! test262 conformance integration test
//!
//! Run with:
//!   cargo test -p quench-test262 --test test262 test262_staged -- --nocapture

use quench_runtime::{builtins, Context, Value};
use quench_test262::host::TestOutcome;
use quench_test262::runner::execute::run_single_test;
use quench_test262::{HarnessLoader, QuenchHost, Test262Host, Test262Runner};
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
fn test_harness_deep_equal_formats_symbol_primitive() {
    let mut host = QuenchHost::new();
    let result =
        host.run_script("assert.sameValue(String(assert.deepEqual.format(Symbol())), 'Symbol()')");
    assert!(result.is_ok(), "Symbol formatting: {:?}", result);
}

#[test]
fn test_harness_deep_equal_symbol_mismatch_throws_object() {
    let mut host = QuenchHost::new();
    let result = host.run_script(
        r#"
        var thrown;
        assert.sameValue(assert.deepEqual._compare(Symbol(), 'Symbol()'), false);
        try { assert.deepEqual(Symbol(), 'Symbol()'); } catch (error) { thrown = error; }
        assert.sameValue(typeof thrown, 'object');
        assert.sameValue(thrown.constructor, Test262Error);
        "#,
    );
    assert!(result.is_ok(), "Symbol mismatch error object: {:?}", result);
}

#[test]
fn test_native_error_constructor_stringifies_function_message() {
    let mut host = QuenchHost::new();
    let result = host.run_script(
        "var f = function () {}; f.toString = function () { return 'custom'; }; var error = new Test262Error(f); assert.sameValue(error.message, 'custom')",
    );
    assert!(result.is_ok(), "function message conversion: {:?}", result);
}

#[test]
fn test_test262_error_constructor_accepts_lazy_formatter_message() {
    let mut host = QuenchHost::new();
    let result = host.run_script(
        "var error = new Test262Error('value: ' + assert.deepEqual.format(Symbol())); assert.sameValue(typeof error, 'object'); assert.sameValue(error.constructor, Test262Error)",
    );
    assert!(result.is_ok(), "lazy Test262Error message: {:?}", result);
}

#[test]
fn test_harness_throws_test262_error_object() {
    let mut host = QuenchHost::new();
    let result = host.run_script(
        "assert.throws(Test262Error, function() { throw new Test262Error('failure'); })",
    );
    assert!(result.is_ok(), "Test262Error throw: {:?}", result);
}

#[test]
fn test_test262_error_global() {
    let mut host = QuenchHost::new();
    let result = host.run_script(
        "assert.sameValue(typeof Test262Error, 'function', 'Test262Error should be a function')",
    );
    assert!(result.is_ok(), "Test262Error global check: {:?}", result);
}

#[test]
fn test_assert_same_value_basic() {
    let mut host = QuenchHost::new();
    let result = host.run_script("assert.sameValue(1, 1, 'one equals one')");
    assert!(result.is_ok(), "sameValue(1,1) should pass: {:?}", result);
}

#[test]
fn test_assert_same_value_nan() {
    let mut host = QuenchHost::new();
    let result = host.run_script("assert.sameValue(NaN, NaN, 'NaN equals NaN')");
    assert!(
        result.is_ok(),
        "sameValue(NaN,NaN) should pass: {:?}",
        result
    );
}

#[test]
fn test_assert_same_value_negative_zero() {
    let mut host = QuenchHost::new();
    let result = host.run_script(
        "assert.sameValue(-0, -0, '-0 equals -0'); assert.sameValue(+0, +0, '+0 equals +0')",
    );
    assert!(result.is_ok(), "sameValue zero: {:?}", result);
}

#[test]
fn test_assert_throws_basic() {
    let mut host = QuenchHost::new();
    let result = host
        .run_script("assert.throws(TypeError, function() { null.x }, 'null.x throws TypeError')");
    assert!(result.is_ok(), "assert.throws should pass: {:?}", result);
}

#[test]
fn test_new_target_in_accessor_invoked_as_member_is_undefined() {
    let mut host = QuenchHost::new();
    let result = host.run_script(
        "var newTarget; var obj = { get m() { newTarget = new.target; } }; obj.m; assert.sameValue(newTarget, undefined)",
    );
    assert!(result.is_ok(), "new.target in accessor: {:?}", result);
}

#[test]
fn test_private_accessor_logical_assignment_calls_setter() {
    let mut host = QuenchHost::new();
    let result = host.run_script(
        "class C { #setterCalledWith; get #field() { return false; } set #field(value) { this.#setterCalledWith = value; } compoundAssignment() { return this.#field ||= true; } setterCalledWithValue() { return this.#setterCalledWith; } } const o = new C(); assert.sameValue(o.compoundAssignment(), true); assert.sameValue(o.setterCalledWithValue(), true)",
    );
    assert!(
        result.is_ok(),
        "private accessor logical assignment: {:?}",
        result
    );
}

#[test]
fn test_derived_class_implicit_constructor_initializes_fields_after_super() {
    let mut host = QuenchHost::new();
    let result = host.run_script(
        "var A = class {}; var C = class extends A { x = 1; }; assert.sameValue(new C().x, 1)",
    );
    assert!(
        result.is_ok(),
        "derived implicit constructor fields: {:?}",
        result
    );
}

#[test]
fn test_direct_eval_in_derived_field_initializer_rejects_super_call() {
    let mut host = QuenchHost::new();
    let result = host.run_script(
        "var A = class {}; var C = class extends A { x = eval('() => super();'); }; assert.throws(SyntaxError, function() { new C().x(); })",
    );
    assert!(
        result.is_ok(),
        "derived field eval super call: {:?}",
        result
    );
}

#[test]
fn test_bind_native_function_does_not_require_derived_super() {
    let mut host = QuenchHost::new();
    let result =
        host.run_script("var bound = parseInt.bind(null); assert.sameValue(bound('7'), 7)");
    assert!(result.is_ok(), "binding native function: {:?}", result);
}

#[test]
fn test_harness_then_strict_async_super_await_returns_string() {
    let mut ctx = Context::new().unwrap();
    builtins::register_builtins(&mut ctx);
    quench_test262::harness::try_inject_harness(&mut ctx).unwrap();
    let _ = ctx.eval(
        r#"
        "use strict";
        var sup = { method() { return 'sup'; } };
        var child = { async method() { var x = await super.method(); return x; } };
        Object.setPrototypeOf(child, sup);
        var result;
        child.method().then(function(value) { result = value; });
        result;
        "#,
    );
    let _ = quench_runtime::builtins::promise::execute_pending_microtasks();
    let result = ctx.eval("result");
    assert_eq!(result, Ok(Value::String("sup".to_string())));
}

#[test]
fn test_switch_scope_lex_async_function_throws_reference_error() {
    let mut host = QuenchHost::new();
    let result = host.run_script("switch (0) { default: async function x() {} } x;");
    assert!(
        result.is_err(),
        "switch default async function declaration should not be visible: {:?}",
        result
    );
}

#[test]
fn test_for_in_with_defined_property() {
    let mut host = QuenchHost::new();
    let result = host.run_script(
        "var o = {a: 1, b: 2}; var keys = []; for (var k in o) { keys.push(k) } assert.sameValue(keys.length, 2)",
    );
    assert!(result.is_ok());
}

// ── Per-iteration let/const binding (spec §14.7.1.1) ─────────────────────────

#[test]
fn test_for_loop_let_per_iteration_basic() {
    let mut host = QuenchHost::new();
    let result = host.run_script(
        r#"
        var result = [];
        for (let i = 0; i < 3; i++) {
            result.push(function() { return i; });
        }
        assert.sameValue(result[0](), 0, "first closure sees i=0");
        assert.sameValue(result[1](), 1, "second closure sees i=1");
        assert.sameValue(result[2](), 2, "third closure sees i=2");
        "#,
    );
    assert!(
        result.is_ok(),
        "per-iteration let binding failed: {:?}",
        result
    );
}

#[test]
fn test_for_loop_let_per_iteration_multiple() {
    let mut host = QuenchHost::new();
    let result = host.run_script(
        r#"
        var result = [];
        for (let i = 0, j = 10; i < 3; i++, j++) {
            result.push(function() { return i + j; });
        }
        assert.sameValue(result[0](), 10, "i=0, j=10 → 10");
        assert.sameValue(result[1](), 12, "i=1, j=11 → 12");
        assert.sameValue(result[2](), 14, "i=2, j=12 → 14");
        "#,
    );
    assert!(result.is_ok(), "multiple let bindings failed: {:?}", result);
}

#[test]
fn test_for_loop_let_closure_sees_body_value() {
    let mut host = QuenchHost::new();
    let result = host.run_script(
        r#"
        var results = [];
        for (let n = 1; n <= 2; n++) {
            var captured = n;
            results.push(function() { return captured; });
        }
        assert.sameValue(results[0](), 2);
        assert.sameValue(results[1](), 2);
        "#,
    );
    assert!(result.is_ok(), "var closure test failed: {:?}", result);
}

// ── Test isolation regression ────────────────────────────────────────────────

#[test]
fn test_reset_interpreter_state_clears_control_flow() {
    quench_runtime::interpreter::reset_interpreter_state();
    assert!(
        quench_runtime::interpreter::take_control_flow().is_none(),
        "control flow should be None after reset"
    );
    assert!(
        !quench_runtime::interpreter::is_strict_mode(),
        "strict mode should be false after reset"
    );
}

#[test]
fn test_quench_host_state_isolation() {
    let tests = vec![
        "var x = 1;",
        "for (var i = 0; i < 3; i++) { }",
        "var a = []; for (let i = 0; i < 3; i++) { a.push(i); }",
        "try { throw new Error('test'); } catch(e) { }",
        "(function() { return 42; })()",
    ];
    let mut host = QuenchHost::new();
    for (i, test) in tests.iter().enumerate() {
        let result = host.run_script(test);
        assert!(result.is_ok(), "test {} should pass: {:?}", i, result);
    }
}

#[test]
fn eval_using_block_preserves_prior_completion_value() {
    let mut ctx = Context::new().expect("context");
    builtins::register_builtins(&mut ctx);
    let value = ctx
        .eval("eval('4; {using resource = null; }')")
        .expect("eval");
    assert_eq!(value, Value::Number(4.0));
}

#[test]
fn test262_using_completion_value_with_harness() {
    let mut host = QuenchHost::new();
    let result = host.run_script("assert.sameValue(eval('4; {using resource = null;}'), 4);");
    assert!(
        result.is_ok(),
        "using completion value failed: {:?}",
        result
    );
}

#[test]
fn using_rethrows_single_disposal_error_as_is() {
    let mut host = QuenchHost::new();
    let result = host.run_script(
        "class MyError extends Error {};
         assert.throws(MyError, function() { throw new MyError(); });
         assert.throws(MyError, function() {
             using resource = { [Symbol.dispose]() { throw new MyError(); } };
         });",
    );
    assert!(
        result.is_ok(),
        "single disposal error changed: {:?}",
        result
    );
}

#[test]
fn using_rejects_non_disposable_initializers() {
    let mut host = QuenchHost::new();
    let result = host.run_script(
        "assert.throws(TypeError, function() { using resource = true; });\
         assert.throws(TypeError, function() { using resource = { [Symbol.dispose]: null }; });\
         assert.throws(TypeError, function() { using resource = {}; });",
    );
    assert!(
        result.is_ok(),
        "invalid using initializer accepted: {:?}",
        result
    );
}

#[test]
fn using_bindings_reject_assignment_in_for_statements() {
    let mut host = QuenchHost::new();
    let result = host.run_script(
        "assert.throws(TypeError, function() { for (using i = null; i === null; i = { [Symbol.dispose]() {} }) {} });\
         assert.throws(TypeError, function() { for (using x of [null]) { x = { [Symbol.dispose]() {} }; } });",
    );
    assert!(
        result.is_ok(),
        "using assignment was accepted: {:?}",
        result
    );
}

#[test]
fn object_method_preserves_subclassed_error_identity() {
    let mut host = QuenchHost::new();
    let result = host.run_script(
        "class MyError extends Error {};
         assert.throws(MyError, function() {
             var dispose = function() { throw new MyError(); };
             dispose();
         });",
    );
    assert!(result.is_ok(), "object method error changed: {:?}", result);
}

#[test]
fn derived_error_constructor_preserves_subclass_prototype() {
    let mut host = QuenchHost::new();
    let result = host.run_script(
        "class MyError extends Error {};\
         var error = new MyError('message');\
         assert.sameValue(error.constructor, MyError);\
         assert.sameValue(error instanceof MyError, true);",
    );
    assert!(
        result.is_ok(),
        "derived Error prototype changed: {:?}",
        result
    );
}

#[test]
fn thrown_derived_error_preserves_constructor_identity() {
    let mut host = QuenchHost::new();
    let result = host.run_script(
        "class MyError extends Error {};\
         function fail() { throw new MyError('message'); }\
         try { fail(); } catch (error) { assert.sameValue(error.constructor, MyError); }",
    );
    assert!(
        result.is_ok(),
        "thrown Error identity changed: {:?}",
        result
    );
}

#[test]
fn get_own_property_names_excludes_symbol_keys() {
    let mut host = QuenchHost::new();
    let result = host.run_script(
        "var sym = Symbol('key');
         var object = { a: 1, [sym]: 2, c: 3 };
         assert.compareArray(Object.getOwnPropertyNames(object), ['a', 'c']);",
    );
    assert!(result.is_ok(), "symbol key leaked into names: {:?}", result);
}

#[test]
fn harness_exposes_global_compare_array() {
    let mut host = QuenchHost::new();
    let result = host.run_script("assert.sameValue(compareArray([1, 2], [1, 2]), true);");
    assert!(result.is_ok(), "global compareArray missing: {:?}", result);
}

#[test]
fn computed_class_constructor_method_replaces_default_constructor_property() {
    let mut host = QuenchHost::new();
    let result = host.run_script(
        "class C { ['constructor']() { return 1; } }
         assert(C !== C.prototype.constructor);
         assert.sameValue(new C().constructor(), 1);",
    );
    assert!(
        result.is_ok(),
        "computed constructor method failed: {:?}",
        result
    );
}

#[test]
fn object_literal_method_super_uses_object_prototype() {
    let mut host = QuenchHost::new();
    let result = host.run_script(
        "var proto = { value: 41 }; var object = { get value() { return super.value + 1; } }; Object.setPrototypeOf(object, proto); assert.sameValue(object.value, 42);",
    );
    assert!(result.is_ok(), "object literal super failed: {:?}", result);
}

#[test]
fn computed_number_property_uses_ecmascript_number_string() {
    let mut host = QuenchHost::new();
    let result =
        host.run_script("var object = { [1e55]: 'B' }; assert.sameValue(object['1e+55'], 'B');");
    assert!(result.is_ok(), "computed number key changed: {:?}", result);
}

#[test]
fn inherited_getter_boxes_primitive_receiver_in_non_strict_call() {
    let mut host = QuenchHost::new();
    let result = host.run_script(
        "Object.defineProperty(Object.prototype, 'x', { get: function() { return this; } }); assert.sameValue((5).x == 5, true);",
    );
    assert!(
        result.is_ok(),
        "primitive getter receiver changed: {:?}",
        result
    );
}

#[test]
fn nested_function_in_strict_function_captures_strictness() {
    let mut host = QuenchHost::new();
    let result = host.run_script(
        "function f1() { 'use strict'; function f() { return typeof this; } return f() === 'undefined' && typeof this === 'undefined'; } assert.sameValue(f1(), true);",
    );
    assert!(
        result.is_ok(),
        "nested function strictness changed: {:?}",
        result
    );
}

#[test]
fn recursive_function_declaration_reaches_base_case() {
    let mut host = QuenchHost::new();
    let result = host.run_script(
        "var y;\nfunction f(a){\n  var x;\n  if (a === 1)\n    return x;\n  else {\n    if(x === undefined) {\n      x = 0;\n    } else {\n      x = 1;\n    }\n    return f(1);\n  }\n}\ny = f(0);\nassert.sameValue(y, undefined);",
    );
    assert!(
        result.is_ok(),
        "recursive function did not terminate: {:?}",
        result
    );
}

#[test]
fn arguments_index_properties_have_default_data_descriptors() {
    let mut host = QuenchHost::new();
    let result = host.run_script(
        "var args = (function() { return arguments; })(1); var descriptor = Object.getOwnPropertyDescriptor(args, '0'); assert.sameValue(descriptor.value, 1); assert.sameValue(descriptor.writable, true); assert.sameValue(descriptor.enumerable, true); assert.sameValue(descriptor.configurable, true);",
    );
    assert!(result.is_ok(), "arguments descriptor changed: {:?}", result);
}

#[test]
fn arguments_index_descriptor_ignores_prototype_accessor() {
    let mut host = QuenchHost::new();
    let result = host.run_script(
        "var data = 'data'; var getFunc = function() { return data; }; var setFunc = function(v) { data = v; }; Object.defineProperty(Object.prototype, '0', { get: getFunc, set: setFunc, configurable: true }); var args = (function() { return arguments; })(1); verifyProperty(args, '0', { value: 1, writable: true, enumerable: true, configurable: true });",
    );
    assert!(
        result.is_ok(),
        "arguments prototype accessor leaked: {:?}",
        result
    );
}

#[test]
fn arguments_descriptor_survives_preceding_mapped_arguments_test() {
    let mut host = QuenchHost::new();
    let result = host.run_script(
        "function foo(a, b, c) { arguments[0] = 1; arguments[1] = 'str'; arguments[2] = 2.1; return a === 1 && b === 'str' && c === 2.1; } assert.sameValue(foo(10, 'sss', 1), true); var data = 'data'; var getFunc = function() { return data; }; var setFunc = function(v) { data = v; }; Object.defineProperty(Object.prototype, '0', { get: getFunc, set: setFunc, configurable: true }); var args = (function() { return arguments; })(1); verifyProperty(args, '0', { value: 1, writable: true, enumerable: true, configurable: true });",
    );
    assert!(result.is_ok(), "stage state leaked: {:?}", result);
}

#[test]
fn arguments_callee_is_non_enumerable_and_writable() {
    let mut host = QuenchHost::new();
    let result = host.run_script(
        "function testcase() { var desc = Object.getOwnPropertyDescriptor(arguments, 'callee'); assert.sameValue(desc.configurable, true); assert.sameValue(desc.enumerable, false); assert.sameValue(desc.writable, true); assert.sameValue(desc.hasOwnProperty('get'), false); } testcase();",
    );
    assert!(
        result.is_ok(),
        "arguments callee descriptor changed: {:?}",
        result
    );
}

#[test]
fn exact_arguments_descriptor_test_source_passes_in_unit_host() {
    let mut host = QuenchHost::new();
    let source =
        include_str!("../../../tests/test262/test/language/arguments-object/10.6-11-b-1.js");
    let result = host.run_script(source);
    assert!(result.is_ok(), "exact arguments test failed: {:?}", result);
}

#[test]
fn exact_arguments_descriptor_test_matches_process_runner_setup() {
    let mut ctx = Context::new().unwrap();
    builtins::register_builtins(&mut ctx);
    quench_test262::harness::try_inject_harness(&mut ctx).unwrap();
    if let Some(error) = ctx.get_global("Test262Error") {
        quench_runtime::value::error::set_main_realm_test262_error(error);
    }
    quench_runtime::interpreter::reset_interpreter_state();
    let source =
        include_str!("../../../tests/test262/test/language/arguments-object/10.6-11-b-1.js");
    assert!(ctx.eval(source).is_ok());
}

#[test]
fn arguments_caller_fixture_parses_no_strict_flag() {
    let source =
        include_str!("../../../tests/test262/test/language/arguments-object/10.6-13-a-2.js");
    let metadata = quench_test262::metadata::Test262Metadata::parse(source).unwrap();
    assert!(metadata.flags.iter().any(|flag| flag == "noStrict"));
}

#[test]
fn arguments_caller_no_strict_fixture_runs_sloppy() {
    let mut host = QuenchHost::new();
    let source =
        include_str!("../../../tests/test262/test/language/arguments-object/10.6-13-a-2.js");
    assert!(host.run_script(source).is_ok());
}

#[test]
fn mapped_arguments_accessor_redefinition_unmaps_index() {
    let mut host = QuenchHost::new();
    let source = include_str!("../../../tests/test262/test/language/arguments-object/mapped/enumerable-configurable-accessor-descriptor.js");
    assert!(host.run_script(source).is_ok());
}

#[test]
fn mapped_arguments_nonconfigurable_property_preserves_mapping() {
    let mut host = QuenchHost::new();
    let source = include_str!("../../../tests/test262/test/language/arguments-object/mapped/mapped-arguments-nonconfigurable-2.js");
    assert!(host.run_script(source).is_ok());
}

#[test]
fn mapped_arguments_nonconfigurable_set_updates_parameter_and_argument() {
    let mut host = QuenchHost::new();
    let result = host.run_script(
        "function f(a) { Object.defineProperty(arguments, '0', {configurable: false}); a = 2; assert.sameValue(a, 2, 'parameter'); assert.sameValue(arguments[0], 2, 'argument'); } f(1);",
    );
    assert!(result.is_ok(), "mapping write changed: {:?}", result);
}

#[test]
fn mapped_arguments_nonconfigurable_descriptor_stays_writable() {
    let mut host = QuenchHost::new();
    let result = host.run_script(
        "function f(a) { Object.defineProperty(arguments, '0', {configurable: false}); var d = Object.getOwnPropertyDescriptor(arguments, '0'); assert.sameValue(d.writable, true); } f(1);",
    );
    assert!(
        result.is_ok(),
        "descriptor writability changed: {:?}",
        result
    );
}

#[test]
fn mapped_arguments_rejects_reconfiguring_nonconfigurable_index() {
    let mut host = QuenchHost::new();
    let source = include_str!("../../../tests/test262/test/language/arguments-object/mapped/nonconfigurable-descriptors-define-failure.js");
    let script = HarnessLoader::new("tests/test262")
        .build_script(source, &[])
        .unwrap();
    let result = host.run_script(&script);
    assert!(result.is_ok(), "reconfiguration was accepted: {:?}", result);
}

#[test]
fn mapped_arguments_rejects_data_redefinition_of_nonconfigurable_accessor() {
    let mut host = QuenchHost::new();
    let result = host.run_script("(function(a) { Object.defineProperty(arguments, '1', {get: () => 3, configurable: false}); assert.throws(TypeError, () => { Object.defineProperty(arguments, '1', {value: 'foo'}); }); })(0);");
    assert!(
        result.is_ok(),
        "accessor redefinition was accepted: {:?}",
        result
    );
}

#[test]
fn mapped_arguments_rejects_strict_delete_of_nonconfigurable_accessor() {
    let mut host = QuenchHost::new();
    let result = host.run_script("(function(a) { Object.defineProperty(arguments, '1', {get: () => 3, configurable: false}); assert.throws(TypeError, () => { 'use strict'; delete arguments[1]; }); })(0);");
    assert!(result.is_ok(), "strict delete was accepted: {:?}", result);
}

#[test]
fn mapped_arguments_descriptor_tracks_index_assignment() {
    let mut host = QuenchHost::new();
    let source = include_str!("../../../tests/test262/test/language/arguments-object/mapped/nonconfigurable-descriptors-set-value-by-arguments.js");
    let result = host.run_script(source);
    assert!(
        result.is_ok(),
        "mapped descriptor value was stale: {:?}",
        result
    );
}

#[test]
fn mapped_arguments_get_own_descriptor_reads_updated_value() {
    let mut host = QuenchHost::new();
    let result = host.run_script("(function(a) { Object.defineProperty(arguments, '0', {configurable: false}); arguments[0] = 2; assert.sameValue(Object.getOwnPropertyDescriptor(arguments, '0').value, 2); })(1);");
    assert!(result.is_ok(), "descriptor did not update: {:?}", result);
}

#[test]
fn mapped_arguments_preserves_non_enumerable_non_writable_descriptor() {
    let mut host = QuenchHost::new();
    let source = include_str!("../../../tests/test262/test/language/arguments-object/mapped/nonconfigurable-nonenumerable-nonwritable-descriptors-basic.js");
    let result = host.run_script(source);
    assert!(
        result.is_ok(),
        "descriptor attributes changed: {:?}",
        result
    );
}

#[test]
fn mapped_arguments_direct_descriptor_preserves_enumerability() {
    let mut host = QuenchHost::new();
    let result = host.run_script("(function(a) { Object.defineProperty(arguments, '0', {configurable: false, enumerable: false, writable: false}); assert.sameValue(Object.getOwnPropertyDescriptor(arguments, '0').enumerable, false); })(1);");
    assert!(result.is_ok(), "enumerability changed: {:?}", result);
}

#[test]
fn eval_code_rejects_arguments_declaration_after_arguments_parameter() {
    let mut host = QuenchHost::new();
    let source = include_str!("../../../tests/test262/test/language/eval-code/direct/arrow-fn-a-following-parameter-is-named-arguments-arrow-func-declare-arguments-assign-incl-def-param-arrow-arguments.js");
    let result = host.run_script(source);
    assert!(
        result.is_ok(),
        "invalid eval declaration was accepted: {:?}",
        result
    );
}

#[test]
fn eval_var_binding_in_default_is_visible_to_arrow_default() {
    let mut host = QuenchHost::new();
    let result = host.run_script("const f = (p = eval(\"var arguments = 'param'\"), q = () => arguments) => q(); assert.sameValue(f(), 'param');");
    assert!(
        result.is_ok(),
        "eval binding was not captured: {:?}",
        result
    );
}

#[test]
fn eval_var_binding_in_default_survives_body_function_declaration() {
    let mut host = QuenchHost::new();
    let result = host.run_script("const f = (p = eval(\"var arguments = 'param'\"), q = () => arguments) => { function arguments() {} assert.sameValue(q(), 'param'); }; f();");
    assert!(result.is_ok(), "eval binding was hidden: {:?}", result);
}

#[test]
fn eval_var_binding_arrow_runs_before_body_hoisting() {
    let mut host = QuenchHost::new();
    let result = host.run_script("const f = (p = eval(\"var arguments = 'param'\"), q = () => arguments, r = q()) => { function arguments() {} assert.sameValue(r, 'param'); }; f();");
    assert!(
        result.is_ok(),
        "default arrow saw wrong binding: {:?}",
        result
    );
}

#[test]
fn eval_var_binding_arrow_ignores_other_body_function_declarations() {
    let mut host = QuenchHost::new();
    let result = host.run_script("const f = (p = eval(\"var arguments = 'param'\"), q = () => arguments) => { function other() {} assert.sameValue(q(), 'param'); }; f();");
    assert!(
        result.is_ok(),
        "unrelated body declaration changed binding: {:?}",
        result
    );
}

#[test]
fn descriptor_object_reads_true_configurable_flag() {
    let mut host = QuenchHost::new();
    let result =
        host.run_script("var d = {configurable: true}; assert.sameValue(d.configurable, true);");
    assert!(result.is_ok(), "descriptor flag failed: {:?}", result);
}

#[test]
fn ordinary_property_rejects_reconfiguring_nonconfigurable_property() {
    let mut host = QuenchHost::new();
    let result = host.run_script("var o = {}; Object.defineProperty(o, 'x', {configurable: false}); assert.throws(TypeError, function() { Object.defineProperty(o, 'x', {configurable: true}); });");
    assert!(
        result.is_ok(),
        "ordinary reconfiguration was accepted: {:?}",
        result
    );
}

#[test]
fn mapped_arguments_assignment_updates_parameters() {
    let mut host = QuenchHost::new();
    let result = host.run_script(
        "function foo(a, b, c) { arguments[0] = 1; arguments[1] = 'str'; arguments[2] = 2.1; return a === 1 && b === 'str' && c === 2.1; } assert.sameValue(foo(10, 'sss', 1), true);",
    );
    assert!(result.is_ok(), "mapped arguments changed: {:?}", result);
}

// ── Runner-path reproduction ─────────────────────────────────────────────────

/// Replicate the EXACT runner path: run_single_test -> run_prepared ->
/// build_script -> run_with_timeout -> execute_script -> run_script.
/// This tests the exact code path the digest runner uses.
#[test]
fn test_runner_path_per_iteration_binding() {
    let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let repo_root = manifest_dir.parent().unwrap().parent().unwrap();
    let test262_dir = repo_root.join("tests/test262");
    let harness = HarnessLoader::new(test262_dir.to_str().unwrap());
    let test_path = test262_dir.join(
        "test/language/statements/let/syntax/\
         let-iteration-variable-is-freshly-allocated-for-each-iteration-single-let-binding.js",
    );

    let outcome = run_single_test(&harness, &test_path);
    match outcome {
        TestOutcome::Pass => {} // good
        TestOutcome::Fail { failure } => {
            panic!(
                "runner path failed: {} (type={:?})",
                failure.message, failure.error_type
            );
        }
        TestOutcome::Skip { reason } => {
            panic!("runner path skipped: {}", reason);
        }
    }
}

/// Run multiple per-iteration tests through the runner path back-to-back.
#[test]
fn test_runner_path_multi_let_per_iteration() {
    let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let repo_root = manifest_dir.parent().unwrap().parent().unwrap();
    let test262_dir = repo_root.join("tests/test262");
    let harness = HarnessLoader::new(test262_dir.to_str().unwrap());
    let tests = vec![
        "let-iteration-variable-is-freshly-allocated-for-each-iteration-single-let-binding.js",
        "let-iteration-variable-is-freshly-allocated-for-each-iteration-multi-let-binding.js",
        "let-closure-inside-condition.js",
    ];

    for name in &tests {
        let test_path = test262_dir
            .join("test/language/statements/let/syntax")
            .join(name);
        let outcome = run_single_test(&harness, &test_path);
        match outcome {
            TestOutcome::Pass => {}
            TestOutcome::Fail { failure } => {
                panic!(
                    "{} failed: {} (type={:?})",
                    name, failure.message, failure.error_type
                );
            }
            TestOutcome::Skip { reason } => {
                panic!("{} skipped: {}", name, reason);
            }
        }
    }
}

#[test]
fn test_runner_path_let_closure_inside_initialization() {
    let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let test262_dir = manifest_dir
        .parent()
        .unwrap()
        .parent()
        .unwrap()
        .join("tests/test262");
    let harness = HarnessLoader::new(test262_dir.to_str().unwrap());
    let test_path = test262_dir
        .join("test/language/statements/let/syntax/let-closure-inside-initialization.js");

    assert_eq!(run_single_test(&harness, &test_path), TestOutcome::Pass);
}

#[test]
fn test_runner_path_var_binding_resolves_before_initializer() {
    let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let test262_dir = manifest_dir
        .parent()
        .unwrap()
        .parent()
        .unwrap()
        .join("tests/test262");
    let harness = HarnessLoader::new(test262_dir.to_str().unwrap());
    let test_path = test262_dir.join("test/language/statements/variable/binding-resolution.js");

    assert_eq!(run_single_test(&harness, &test_path), TestOutcome::Pass);
}

#[test]
fn test_runner_path_eval_var_arguments_is_allowed() {
    let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let test262_dir = manifest_dir
        .parent()
        .unwrap()
        .parent()
        .unwrap()
        .join("tests/test262");
    let harness = HarnessLoader::new(test262_dir.to_str().unwrap());
    let test_path = test262_dir.join("test/language/statements/variable/12.2.1-11.js");

    assert_eq!(run_single_test(&harness, &test_path), TestOutcome::Pass);
}

#[test]
fn test_runner_path_with_function_this_binding() {
    let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let test262_dir = manifest_dir
        .parent()
        .unwrap()
        .parent()
        .unwrap()
        .join("tests/test262");
    let harness = HarnessLoader::new(test262_dir.to_str().unwrap());
    let test_path = test262_dir.join("test/language/statements/with/S12.10_A1.7_T1.js");

    assert_eq!(run_single_test(&harness, &test_path), TestOutcome::Pass);
}

#[test]
fn test_runner_path_using_completion_value() {
    let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let test262_dir = manifest_dir
        .parent()
        .unwrap()
        .parent()
        .unwrap()
        .join("tests/test262");
    let harness = HarnessLoader::new(test262_dir.to_str().unwrap());
    let test_path = test262_dir.join("test/language/statements/using/cptn-value.js");

    assert_eq!(run_single_test(&harness, &test_path), TestOutcome::Pass);
}

#[test]
fn module_resolution_negative_case_reports_syntax_error() {
    let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let test262_dir = manifest_dir
        .parent()
        .unwrap()
        .parent()
        .unwrap()
        .join("tests/test262");
    let harness = HarnessLoader::new(test262_dir.to_str().unwrap());
    let path = test262_dir.join("test/language/module-code/instn-iee-err-circular.js");
    let outcome = run_single_test(&harness, &path);
    assert!(
        matches!(outcome, TestOutcome::Pass),
        "module resolution negative case: {outcome:?}"
    );
}

// ── Staged runner ───────────────────────────────────────────────────────────

#[test]
#[ignore = "staged test262 runner"]
fn test262_staged() {
    // Spawn on a thread with a larger stack to avoid stack overflows
    // during deep parsing (default Rust test threads have ~2MB stack).
    let builder = std::thread::Builder::new().stack_size(64 * 1024 * 1024);
    let (tx, rx) = std::sync::mpsc::channel();
    builder
        .spawn(move || {
            let result = std::panic::catch_unwind(test262_staged_impl);
            let _ = tx.send(result);
        })
        .expect("failed to spawn runner thread");
    match rx.recv_timeout(std::time::Duration::from_secs(test262_run_timeout_secs())) {
        Ok(Ok(())) => {}
        Ok(Err(error)) => panic!("runner thread panicked: {:?}", error),
        Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {
            panic!(
                "test262 runner timed out after {}s",
                test262_run_timeout_secs()
            );
        }
        Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => {
            panic!("test262 runner thread disconnected")
        }
    }
}

const DEFAULT_TEST262_RUN_TIMEOUT_SECS: u64 = 1800;

fn test262_run_timeout_secs() -> u64 {
    parse_test262_run_timeout(std::env::var("TEST262_RUN_TIMEOUT_SECS").ok().as_deref())
}

fn parse_test262_run_timeout(value: Option<&str>) -> u64 {
    value
        .and_then(|value| value.parse().ok())
        .filter(|value| *value > 0)
        .unwrap_or(DEFAULT_TEST262_RUN_TIMEOUT_SECS)
}

#[test]
fn staged_runner_timeout_has_bounded_default_and_rejects_invalid_values() {
    assert_eq!(parse_test262_run_timeout(None), 1800);
    assert_eq!(parse_test262_run_timeout(Some("0")), 1800);
    assert_eq!(parse_test262_run_timeout(Some("invalid")), 1800);
    assert_eq!(parse_test262_run_timeout(Some("7")), 7);
}

fn test262_staged_impl() {
    let test262_dir = {
        let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
        let repo_root = manifest_dir.parent().unwrap().parent().unwrap();
        std::env::var("TEST262_DIR")
            .map(PathBuf::from)
            .unwrap_or_else(|_| repo_root.join("tests/test262"))
    };
    let digest = std::env::var("TEST262_DIGEST")
        .ok()
        .map(|s| s == "1" || s.eq_ignore_ascii_case("true"))
        .unwrap_or(false);
    let runner = Test262Runner::new(test262_dir);
    let summary = runner.run();
    if summary.skipped > 0 && !digest {
        let mut reason_counts: std::collections::BTreeMap<&str, usize> =
            std::collections::BTreeMap::new();
        for (_path, reason) in quench_test262::skip::crash_files() {
            *reason_counts.entry(*reason).or_default() += 1;
        }
        panic!(
            "Stage {} incomplete: {} skipped (skips never count as passes). \
             Configured skip reasons: {:?}. Fix the crash or remove the stale skip entry.",
            current_stage_label(),
            summary.skipped,
            reason_counts,
        );
    }
    if summary.failed > 0 {
        if digest {
            std::process::exit(1);
        } else {
            panic!(
                "Stage {} failed: {}/{} passed. First failure: {:?}",
                current_stage_label(),
                summary.passed,
                summary.passed + summary.failed,
                summary.first_failure,
            );
        }
    }
}

fn current_stage_label() -> String {
    std::env::var("TEST262_STAGE")
        .unwrap_or_else(|_| quench_test262::runner::default_stage().to_string())
}

#[test]
fn test262_one() {
    let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let repo_root = manifest_dir.parent().unwrap().parent().unwrap();
    let path = std::env::var("TEST262_FILE")
        .map(PathBuf::from)
        .unwrap_or_else(|_| {
            repo_root.join("tests/test262/test/language/statements/function/S13.2.1_A6_T3.js")
        });
    let test262_dir = std::env::var("TEST262_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|_| repo_root.join("tests/test262"));

    let runner = Test262Runner::new(test262_dir);
    let src = std::fs::read_to_string(path).expect("read test file");
    let meta = quench_test262::metadata::Test262Metadata::parse(&src).unwrap_or_default();
    let mut host = QuenchHost::new();
    let script = runner
        .harness
        .build_script(&src, &meta.includes)
        .expect("build script");
    let start = std::time::Instant::now();
    let result = host.run_script(&script);
    let elapsed = start.elapsed();
    let _ = elapsed;
    match result {
        Ok(()) => {}
        Err(e) => panic!("FAIL: {}", e),
    }
}

// Reproducer: for (let i = 0; i < 2; ++i) {} must terminate (no infinite loop)
#[test]
fn for_loop_with_let_should_terminate() {
    use std::sync::mpsc;
    use std::time::Duration;

    let (tx, rx) = mpsc::channel();
    let handle = std::thread::spawn(move || {
        let mut host = QuenchHost::new();
        let result = host.run_script("for (let i = 0; i < 2; ++i) {}");
        let _ = tx.send(result);
    });

    match rx.recv_timeout(Duration::from_secs(3)) {
        Ok(result) => {
            assert!(result.is_ok(), "for loop must terminate, got: {:?}", result);
        }
        Err(_) => {
            panic!("TIMEOUT: for loop did not terminate in 3s");
        }
    }

    assert!(handle.join().is_ok(), "eval thread panicked");
}

#[test]
fn for_loop_let_body_update_should_terminate() {
    use std::sync::mpsc;
    use std::time::Duration;

    let (tx, rx) = mpsc::channel();
    let handle = std::thread::spawn(move || {
        let mut host = QuenchHost::new();
        let result = host.run_script("var x = 0; for (let y = 0; y < 5; ) { y++; x++; } x");
        let _ = tx.send(result);
    });

    match rx.recv_timeout(Duration::from_secs(3)) {
        Ok(result) => {
            assert!(
                result.is_ok(),
                "for loop with body update must terminate, got: {:?}",
                result
            );
        }
        Err(_) => {
            panic!("TIMEOUT: for loop with body update did not terminate in 3s");
        }
    }

    assert!(handle.join().is_ok(), "eval thread panicked");
}

#[test]
fn for_loop_let_per_iteration_binding_closure() {
    let mut ctx = quench_runtime::Context::new().unwrap();
    quench_runtime::builtins::register_builtins(&mut ctx);
    let r = ctx.eval(
        r#"
        "use strict";
        var a = [];
        for (let i = 0; i < 5; ++i) {
            a.push(function() { return i; });
        }
        var results = a.map(function(f) { return f(); });
        var pass = true;
        for (var k = 0; k < 5; ++k) {
            if (results[k] !== k) pass = false;
        }
        pass
        "#,
    );
    assert!(r.is_ok(), "eval failed: {:?}", r);
    let v = r.as_ref().unwrap();
    assert!(
        quench_runtime::value::coerce::to_bool(v),
        "closures should capture per-iteration i values, got: {:?}",
        v
    );
}

#[test]
fn for_loop_multi_let_per_iteration_binding() {
    let mut ctx = quench_runtime::Context::new().unwrap();
    quench_runtime::builtins::register_builtins(&mut ctx);
    let r = ctx.eval(
        r#"
        "use strict";
        var a = [];
        for (let i = 0, j = 10; i < 3; ++i, ++j) {
            a.push(function() { return i * 100 + j; });
        }
        var pass = true;
        if (a[0]() !== 10) pass = false;
        if (a[1]() !== 111) pass = false;
        if (a[2]() !== 212) pass = false;
        pass
        "#,
    );
    assert!(r.is_ok(), "eval failed: {:?}", r);
    let v = r.as_ref().unwrap();
    assert!(
        quench_runtime::value::coerce::to_bool(v),
        "closures should capture per-iteration i,j values, got: {:?}",
        v
    );
}

#[test]
fn delete_class_static_method_removes_property() {
    let mut ctx = quench_runtime::Context::new().unwrap();
    quench_runtime::builtins::register_builtins(&mut ctx);
    let result = ctx
        .eval("class C { static m() {} } delete C.m && C.m === undefined")
        .unwrap();
    assert_eq!(result, quench_runtime::Value::Boolean(true));
}

#[test]
fn object_spread_copies_symbol_properties() {
    let mut ctx = quench_runtime::Context::new().unwrap();
    quench_runtime::builtins::register_builtins(&mut ctx);
    let result = ctx
        .eval("var s = Symbol('s'), o = {}; o[s] = 1; ({...o})[s] === 1")
        .unwrap();
    assert_eq!(result, quench_runtime::Value::Boolean(true));
}

#[test]
fn generator_identity_is_preserved_by_destructuring_assignment() {
    let mut ctx = quench_runtime::Context::new().unwrap();
    quench_runtime::builtins::register_builtins(&mut ctx);
    let result = ctx
        .eval("function* g() { yield 1; } var iterator = g(); var result; result = [,] = iterator; result === iterator")
        .unwrap();
    assert_eq!(result, quench_runtime::Value::Boolean(true));
}

#[test]
fn arrow_field_super_assignment_uses_class_receiver() {
    let mut ctx = quench_runtime::Context::new().unwrap();
    quench_runtime::builtins::register_builtins(&mut ctx);
    let result = ctx
        .eval("class C { func = () => { super.prop = 'ok'; }; } var c = new C(); c.func(); c.prop")
        .unwrap();
    assert_eq!(result, quench_runtime::Value::String("ok".into()));
}

#[test]
fn arrow_field_captures_instance_this() {
    let mut ctx = quench_runtime::Context::new().unwrap();
    quench_runtime::builtins::register_builtins(&mut ctx);
    let result = ctx
        .eval("class C { func = () => this; } var c = new C(); c.func() === c")
        .unwrap();
    assert_eq!(result, quench_runtime::Value::Boolean(true));
}

#[test]
fn anonymous_arrow_has_configurable_empty_name_property() {
    let mut ctx = quench_runtime::Context::new().unwrap();
    quench_runtime::builtins::register_builtins(&mut ctx);
    let result = ctx
        .eval("var d = Object.getOwnPropertyDescriptor(() => {}, 'name'); d.value === '' && d.configurable === true && d.writable === false && d.enumerable === false")
        .unwrap();
    assert_eq!(result, quench_runtime::Value::Boolean(true));
}

#[test]
fn anonymous_arrow_reports_name_as_own_property() {
    let mut ctx = quench_runtime::Context::new().unwrap();
    quench_runtime::builtins::register_builtins(&mut ctx);
    let result = ctx
        .eval("Object.prototype.hasOwnProperty.call(() => {}, 'name')")
        .unwrap();
    assert_eq!(result, quench_runtime::Value::Boolean(true));
}

#[test]
fn anonymous_arrow_name_can_be_deleted() {
    let mut ctx = quench_runtime::Context::new().unwrap();
    quench_runtime::builtins::register_builtins(&mut ctx);
    let result = ctx
        .eval("var f = () => {}; var before = Object.prototype.hasOwnProperty.call(f, 'name'); var deleted = delete f.name; var after = Object.prototype.hasOwnProperty.call(f, 'name'); before && deleted && !after")
        .unwrap();
    assert_eq!(result, quench_runtime::Value::Boolean(true));
}

#[test]
fn restored_class_async_generator_next_resolves() {
    let mut ctx = quench_runtime::Context::new().unwrap();
    quench_runtime::builtins::register_builtins(&mut ctx);
    ctx.eval(
        "var result; class C { static async *m() { return 42; } } \
         C.m().next().then(function(value) { result = value.value; });",
    )
    .unwrap();
    let _ = quench_runtime::builtins::promise::execute_pending_microtasks();
    assert_eq!(
        ctx.get_global("result"),
        Some(quench_runtime::Value::Number(42.0))
    );
}

#[test]
fn restored_class_async_generator_chained_then_calls_done() {
    let mut ctx = quench_runtime::Context::new().unwrap();
    quench_runtime::builtins::register_builtins(&mut ctx);
    ctx.eval(
        "var done = 0; function $DONE() { done++; } \
         class C { static async *m() { return 42; } } \
         var d = Object.getOwnPropertyDescriptor(C, 'm'); \
         delete C.m; Object.defineProperty(C, 'm', d); \
         C.m().next().then(function(value) { return Promise.resolve(value); }) \
         .then($DONE, $DONE);",
    )
    .unwrap();
    let _ = quench_runtime::builtins::promise::execute_pending_microtasks();
    assert_eq!(
        ctx.get_global("done"),
        Some(quench_runtime::Value::Number(1.0))
    );
}
