//! test262 conformance harness for quench-runtime
//!
//! This module provides a harness to run ECMAScript test262 tests against
//! the quench-runtime. It parses test262 frontmatter, implements skip policy,
//! and provides minimal harness helpers as Rust native functions.

pub mod metadata;
pub mod harness;
pub mod runner;

pub use metadata::Test262Metadata;
pub use runner::{TestOutcome, TestResult, Test262Report, should_skip, run_test_file, run_suite, write_report};
