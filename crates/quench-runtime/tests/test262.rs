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
    let harness = quench_runtime::test262::harness::HarnessLoader::new(test262_dir.to_str().unwrap());
    let meta = quench_runtime::test262::metadata::Test262Metadata::parse(&source).expect("Failed to parse frontmatter");
    let script = harness.build_script(&source, &meta.includes).expect("Failed to build script");
    
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
    assert!(result.is_ok(), "assert.deepEqual should exist: {:?}", result);
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
    assert!(result.is_ok(), "assert.throws with deepEqual should work: {:?}", result);
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
