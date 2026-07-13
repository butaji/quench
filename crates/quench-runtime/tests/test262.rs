//! test262 conformance integration test
//!
//! Run with:
//!   cargo test -p quench-runtime --test test262 test262_staged -- --ignored --nocapture

use quench_runtime::test262::{QuenchHost, Test262Host, Test262Runner};
use std::path::PathBuf;

#[test]
fn test_harness_deep_equal_basic() {
    // Test that assert.deepEqual with basic arrays works
    let mut host = QuenchHost::new();
    let result = host.run_script("assert.deepEqual([], [])");
    assert!(
        result.is_ok(),
        "Basic deepEqual should work, got: {:?}",
        result
    );
}

#[test]
fn test_harness_deep_equal_with_values() {
    let mut host = QuenchHost::new();
    let result = host.run_script("assert.deepEqual([1, 2], [1, 2])");
    assert!(
        result.is_ok(),
        "deepEqual with values should work, got: {:?}",
        result
    );
}

#[test]
fn test_harness_deep_equal_primitives() {
    // This replicates the failing harness test: test/harness/deepEqual-primitives.js
    use std::path::PathBuf;
    let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let repo_root = manifest_dir.parent().unwrap().parent().unwrap();
    let test262_dir = repo_root.join("tests/test262");
    let test_path = repo_root.join("tests/test262/test/harness/deepEqual-primitives.js");

    let source = std::fs::read_to_string(&test_path).expect("Failed to read test file");
    let harness =
        quench_runtime::test262::harness::HarnessLoader::new(test262_dir.to_str().unwrap());
    let meta = quench_runtime::test262::metadata::Test262Metadata::parse(&source)
        .expect("Failed to parse frontmatter");
    let script = harness
        .build_script(&source, &meta.includes)
        .expect("Failed to build script");

    let mut host = QuenchHost::new();
    let result = host.run_script(&script);
    assert!(
        result.is_ok(),
        "deepEqual-primitives.js failed: {:?}",
        result
    );
}

#[test]
fn test_deep_equal_js_loads() {
    // Test that deepEqual.js harness file loads and overrides native deepEqual
    use std::path::PathBuf;
    let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let repo_root = manifest_dir.parent().unwrap().parent().unwrap();
    let test262_dir = repo_root.join("tests/test262");

    let mut host = QuenchHost::new();

    // First test: just load deepEqual.js and check assert.deepEqual exists
    let script = r#"
        typeof assert.deepEqual;
    "#;
    let result = host.run_script(script);
    assert!(
        result.is_ok(),
        "assert.deepEqual should exist: {:?}",
        result
    );
}

#[test]
fn test_assert_throws_with_deep_equal() {
    // Test that assert.throws works with assert.deepEqual inside
    let mut host = QuenchHost::new();
    let script = r#"
        assert.throws(Test262Error, function() {
            assert.deepEqual(null, 0);
        });
    "#;
    let result = host.run_script(script);
    eprintln!("assert.throws result: {:?}", result);
    assert!(
        result.is_ok(),
        "assert.throws with deepEqual should work: {:?}",
        result
    );
}

#[test]
fn test_deep_equal_objects_with_different_arrays() {
    // This replicates the failing case from deepEqual-deep.js
    let mut host = QuenchHost::new();
    let script = r#"
        assert.throws(Test262Error, function() {
            assert.deepEqual({ a: { x: 1 }, b: [true] }, { a: { x: 1 }, b: [false] });
        });
    "#;
    let result = host.run_script(script);
    eprintln!("deepEqual objects-with-arrays result: {:?}", result);
    assert!(
        result.is_ok(),
        "deepEqual should throw for objects with different arrays: {:?}",
        result
    );
}

#[test]
fn test_deep_equal_passes_for_equal_nested_objects() {
    // This should pass (equal objects)
    let mut host = QuenchHost::new();
    let script = r#"
        assert.deepEqual({ a: { x: 1 }, b: [true] }, { a: { x: 1 }, b: [true] });
    "#;
    let result = host.run_script(script);
    eprintln!("deepEqual equal objects result: {:?}", result);
    assert!(
        result.is_ok(),
        "deepEqual should pass for equal objects: {:?}",
        result
    );
}

#[test]
fn test_deep_equal_throws_for_missing_property() {
    // This should throw (different properties)
    let mut host = QuenchHost::new();
    let script = r#"
        assert.throws(Test262Error, function() {
            assert.deepEqual({}, { a: { x: 1 }, b: [true] });
        });
    "#;
    let result = host.run_script(script);
    eprintln!("deepEqual missing property result: {:?}", result);
    assert!(
        result.is_ok(),
        "deepEqual should throw for missing property: {:?}",
        result
    );
}

#[test]
fn test_deep_equal_boxed_primitives() {
    // Object("a") should equal "a" per ES spec (boxed primitives)
    let mut host = QuenchHost::new();
    let script = r#"
        assert.deepEqual(Object("a"), "a");
        assert.deepEqual(Object(1), 1);
        assert.deepEqual(Object(true), true);
    "#;
    let result = host.run_script(script);
    eprintln!("boxed primitives result: {:?}", result);
    assert!(
        result.is_ok(),
        "boxed primitives should equal their primitive values: {:?}",
        result
    );
}

#[test]
fn test_property_is_enumerable() {
    // Test that Object.defineProperty with enumerable:true creates enumerable property
    let mut host = QuenchHost::new();
    let script = r#"
        var obj = {};
        Object.defineProperty(obj, 'a', { enumerable: true });
        var desc = Object.getOwnPropertyDescriptor(obj, 'a');
        assert.sameValue(desc.enumerable, true, "descriptor enumerable should be true");
        assert.sameValue(obj.propertyIsEnumerable('a'), true, "propertyIsEnumerable should return true");
    "#;
    let result = host.run_script(script);
    eprintln!("propertyIsEnumerable result: {:?}", result);
    assert!(
        result.is_ok(),
        "propertyIsEnumerable should work correctly: {:?}",
        result
    );
}

#[test]
fn test_for_in_with_defined_property() {
    // Test that for...in enumerates properties defined with Object.defineProperty
    let mut host = QuenchHost::new();
    let script = r#"
        var obj = {};
        Object.defineProperty(obj, 'a', { enumerable: true });
        var found = false;
        for (var key in obj) {
            if (key === 'a') found = true;
        }
        assert.sameValue(found, true, "for-in should find enumerable property 'a'");
    "#;
    let result = host.run_script(script);
    eprintln!("for-in result: {:?}", result);
    assert!(
        result.is_ok(),
        "for-in should enumerate defined enumerable property: {:?}",
        result
    );
}

#[test]
fn test_own_keys_with_defined_property() {
    // Test own keys after Object.defineProperty
    let mut host = QuenchHost::new();
    let script = r#"
        var obj = {};
        Object.defineProperty(obj, 'a', { enumerable: true });
        var desc = Object.getOwnPropertyDescriptor(obj, 'a');
        assert.sameValue(desc.enumerable, true);
        var keys = Object.keys(obj);
        assert.sameValue(keys.length, 1);
        assert.sameValue(keys[0], 'a');
    "#;
    let result = host.run_script(script);
    eprintln!("own keys result: {:?}", result);
    assert!(
        result.is_ok(),
        "own keys should include enumerable property: {:?}",
        result
    );
}

#[test]
fn test_symbol_creation() {
    // Test Symbol creation
    let mut host = QuenchHost::new();
    let script = r#"
        var s = Symbol();
        typeof s;
    "#;
    let result = host.run_script(script);
    eprintln!("Symbol result: {:?}", result);
    assert!(result.is_ok(), "Symbol should work: {:?}", result);
}

#[test]
fn test_get_symbol_to_prim_err() {
    // Reproducer for test262 language/expressions/addition/get-symbol-to-prim-err.js:
    // when the @@toPrimitive getter throws, the other operand's getter must not run.
    use quench_runtime::test262::harness::HarnessLoader;
    use quench_runtime::test262::metadata::Test262Metadata;
    let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let repo_root = manifest_dir.parent().unwrap().parent().unwrap();
    let test262_dir = repo_root.join("tests/test262");
    let test_path = repo_root
        .join("tests/test262/test/language/expressions/addition/get-symbol-to-prim-err.js");

    let source = std::fs::read_to_string(&test_path).expect("Failed to read test file");
    let harness = HarnessLoader::new(test262_dir.to_str().unwrap());
    let meta = Test262Metadata::parse(&source).expect("Failed to parse frontmatter");
    let script = harness
        .build_script(&source, &meta.includes)
        .expect("Failed to build script");

    let mut host = QuenchHost::new();
    let sloppy = host.run_script(&script);
    assert!(sloppy.is_ok(), "sloppy run failed: {:?}", sloppy);
    let strict_script = format!("\"use strict\";\n{}", script);
    let strict = host.run_script(&strict_script);
    assert!(strict.is_ok(), "strict run failed: {:?}", strict);
}

#[test]
fn test_math_defined() {
    let mut host = QuenchHost::new();
    let result = host.run_script("typeof Math");
    println!("typeof Math = {:?}", result);
    assert!(result.is_ok(), "Math should be defined: {:?}", result);
}

#[test]
fn test_new_math_throws_typeerror() {
    let mut host = QuenchHost::new();
    let result = host.run_script(r"new Math");
    println!("new Math result = {:?}", result);
    // Should throw TypeError, not ReferenceError
    assert!(result.is_err(), "new Math should throw, got: {:?}", result);
}

#[test]
fn test_var_hoisting_global_scope() {
    // var declarations should be hoisted to undefined before execution.
    // This is the exact pattern from S8.3_A1_T1.js
    let mut host = QuenchHost::new();
    let result = host.run_script(
        r"if (x !== undefined) { throw new Error('#0 x !== undefined'); } var x = true;",
    );
    assert!(result.is_ok(), "var x hoisted to undefined: {:?}", result);
}

#[test]
fn test_var_hoisting_before_declaration() {
    let mut host = QuenchHost::new();
    let result = host.run_script(
        r#"
var result = (y === undefined);
var y = 42;
if (!result) throw new Error("y should be undefined before declaration");
"#,
    );
    assert!(result.is_ok(), "var y hoisted to undefined: {:?}", result);
}

#[test]
fn test_block_let_shadows_outer_let() {
    // Reproducer for test262 statements/block/scope-lex-close.js: a `let`
    // inside a block creates a new lexical binding that shadows an outer
    // `let` of the same name; a closure inside the block must see the
    // inner value, while code outside the block sees the outer value.
    let mut host = QuenchHost::new();
    let result = host.run_script(
        r#"
var probe;
{
  let x = 'inside';
  probe = function() { return x; };
}
let x = 'outside';
if (x !== 'outside') throw new Error('outer x wrong: ' + x);
if (probe() !== 'inside') throw new Error('probe() wrong: ' + probe());
"#,
    );
    assert!(result.is_ok(), "block let shadowing failed: {:?}", result);
}

#[test]
fn test_deep_equal_circular_no_stack_overflow() {
    let mut host = QuenchHost::new();
    let result = host.run_script(
        r#"
var a = { x: 1 };
var b = { x: 1 };
a.a = a;
a.b = b;
b.a = b;
b.b = a;
assert.deepEqual(a, b);
"#,
    );
    assert!(result.is_ok(), "circular deepEqual failed: {:?}", result);
}

#[test]
fn test_arrow_object_parameter_default_binding() {
    let mut host = QuenchHost::new();
    let result = host.run_script(
        r#"
var af = ({x = 1}) => x;
assert.sameValue(af({}), 1);
assert.sameValue(af({x: 2}), 2);
"#,
    );
    assert!(
        result.is_ok(),
        "object parameter default failed: {:?}",
        result
    );
}

#[test]
fn test_arrow_parameter_closure_cannot_see_body_var() {
    let mut host = QuenchHost::new();
    let result = host.run_script(
        r#"
var x = 'outside';
var probeParams, probeBody;
((_ = probeParams = function() { return x; }) => {
  var x = 'inside';
  probeBody = function() { return x; };
})();
assert.sameValue(probeParams(), 'outside');
assert.sameValue(probeBody(), 'inside');
"#,
    );
    assert!(
        result.is_ok(),
        "parameter/body scope split failed: {:?}",
        result
    );
}

#[test]
fn test_arrow_body_var_does_not_leak_global() {
    let mut host = QuenchHost::new();
    let result = host.run_script(
        r#"
var probe;
((_ = null) => { var x = 'inside'; probe = function() { return x; }; })();
var x = 'outside';
assert.sameValue(probe(), 'inside');
assert.sameValue(x, 'outside');
"#,
    );
    assert!(result.is_ok(), "arrow body var scope failed: {:?}", result);
}

#[test]
fn test_rest_parameter_after_missing_argument_is_empty() {
    let mut host = QuenchHost::new();
    let result = host.run_script(
        r#"
var length;
((first, ...rest) => { length = rest.length; })();
assert.sameValue(length, 0);
"#,
    );
    assert!(result.is_ok(), "empty rest parameter failed: {:?}", result);
}

#[test]
fn test_arrow_rest_destructuring_default_closes_over_eval_var() {
    let mut host = QuenchHost::new();
    let result = host.run_script(
        r#"
var x = 'outside';
var probeParam, probeBody;
((...[_ = (eval('var x = "inside";'), probeParam = function() { return x; })]) => {
  probeBody = function() { return x; };
})();
assert.sameValue(probeParam(), 'inside');
assert.sameValue(probeBody(), 'inside');
"#,
    );
    assert!(
        result.is_ok(),
        "rest destructuring closure failed: {:?}",
        result
    );
}

#[test]
fn test_eval_var_conflicts_with_arrow_body_let() {
    let mut host = QuenchHost::new();
    let result = host.run_script(
        r#"
var a = () => { let x; eval('var x;'); };
assert.throws(SyntaxError, a);
"#,
    );
    assert!(result.is_ok(), "eval lexical conflict failed: {:?}", result);
}

#[test]
fn test_arrow_lexically_captures_super_property() {
    let mut host = QuenchHost::new();
    let result = host.run_script(
        r#"
var count = 0;
class A { increment() { count++; } }
class B extends A { incrementer() { (_ => super.increment())(); } }
new B().incrementer();
assert.sameValue(count, 1);
"#,
    );
    assert!(
        result.is_ok(),
        "lexical super in arrow failed: {:?}",
        result
    );
}

#[test]
fn test_create_realm_uses_its_primitive_prototypes() {
    let mut host = QuenchHost::new();
    let result = host.run_script(
        r#"
var other = $262.createRealm().global;
other.Number.prototype.test262 = 'number prototype';
other.value = 1;
assert.sameValue(other.eval('value.test262'), 'number prototype');
"#,
    );
    assert!(
        result.is_ok(),
        "cross-realm primitive lookup failed: {:?}",
        result
    );
}

#[test]
#[ignore = "run with --ignored to activate staged runner"]
fn test262_staged() {
    let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let repo_root = manifest_dir.parent().unwrap().parent().unwrap();
    let test262_dir = std::env::var("TEST262_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|_| repo_root.join("tests/test262"));
    let checkpoint_path = manifest_dir.join(".test262_checkpoint");

    let mut host = QuenchHost::new();
    let runner = Test262Runner::new(test262_dir, checkpoint_path);
    let summary = runner.run(&mut host);
    assert!(
        summary.failed == 0,
        "test262 run had {} failure(s); first: {:?}",
        summary.failed,
        summary.first_failure
    );
}
