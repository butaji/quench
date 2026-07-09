//! Regression test template for incremental conformance.
//!
//! Copy this file for each new spec area or individual test file you want to
//! lock in. Rename `test_regression_example` to something descriptive and
//! replace the source with the failing snippet or test262 path.
//!
//! During incremental conformance work:
//!  1. Run the incremental harness to find the next failing file.
//!  2. Copy the failing behavior into a test based on this template.
//!  3. Implement the minimal fix that makes this test pass.
//!  4. Only then move to the next file in deterministic order.

use quench_runtime::{Context, Value};

#[test]
fn test_regression_example() {
    let mut ctx = Context::new().unwrap();

    // Inline JS regression test.
    let result = ctx.eval(
        r#"
        // Replace with the minimal reproduction from the failing test file.
        1 + 1;
    "#,
    );

    assert!(result.is_ok(), "eval failed: {:?}", result);
    assert_eq!(result.unwrap(), Value::Number(2.0));
}

#[test]
#[ignore = "enable once a test262 submodule/file is available"]
fn test_regression_from_test262_file() {
    use std::path::PathBuf;

    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("test262")
        .join("test")
        .join("language")
        .join("example.js");

    quench_runtime::test262::assert_test262_file_passes(&path);
}
