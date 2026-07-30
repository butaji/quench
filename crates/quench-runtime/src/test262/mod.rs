//! test262 conformance harness for quench-runtime
//!
//! Run with:
//!   cargo test -p quench-runtime --test test262 test262_staged -- --nocapture
//!
//! Run a single stage:
//!   TEST262_STAGE=0 cargo test -p quench-runtime --test test262 test262_staged -- --nocapture
//!
//! Run all stages:
//!   ALL_STAGES=1 cargo test -p quench-runtime --test test262 test262_staged -- --nocapture

pub mod harness;
pub mod host;
pub mod metadata;
pub mod runner;
pub mod skip;

pub use harness::HarnessLoader;
pub use host::{
    capture_thrown_diagnostics, read_source_context, QuenchHost, Test262Host, TestFailure,
    TestOutcome,
};
pub use runner::{Test262Runner, STAGES};
