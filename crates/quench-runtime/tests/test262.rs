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
