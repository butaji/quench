//! test262 conformance harness for quench-runtime
//!
//! This module provides a harness to run ECMAScript test262 tests against
//! the quench-runtime. It parses test262 frontmatter, implements skip policy,
//! and provides minimal harness helpers as Rust native functions.

pub mod batches;
pub mod harness;
pub mod harness_tests;
pub mod metadata;
pub mod runner;
pub mod skip;

pub use metadata::Test262Metadata;
pub use runner::{
    TestOutcome, TestResult, Test262Report, assert_test262_file_passes, collect_test_files,
    run_suite, run_suite_stop_on_fail, should_skip, write_report,
};
