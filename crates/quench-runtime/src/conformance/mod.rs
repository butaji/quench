//! Conformance test harness for quench-runtime
//!
//! This module provides harnesses to run conformance test suites:
//! - test262: ECMAScript conformance tests
//! - typescript: TypeScript compiler test cases

pub mod report;

// Re-export test262 from the root module
pub use crate::test262::runner::{TestOutcome, TestResult, Test262Report, run_suite as run_test262_suite};

pub mod typescript;
