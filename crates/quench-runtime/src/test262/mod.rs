//! test262 conformance harness for quench-runtime
//!
//! Run with:
//!   cargo test -p quench-runtime --test test262 test262_staged -- --ignored --nocapture
//!
//! Run a single stage:
//!   TEST262_STAGE=0 cargo test -p quench-runtime --test test262 test262_staged -- --ignored --nocapture
//!
//! Run all stages:
//!   ALL_STAGES=1 cargo test -p quench-runtime --test test262 test262_staged -- --ignored --nocapture

pub mod harness;
pub mod host;
pub mod metadata;
pub mod runner;
pub mod skip;

pub use harness::HarnessLoader;
pub use host::{QuenchHost, Test262Host, TestOutcome};
pub use runner::{Test262Runner, STAGES};
