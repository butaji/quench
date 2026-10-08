//! Node API host backed by the canonical shared Quench runtime.
//!
//! The host boundary owns Node policy and API behavior. JavaScript semantics
//! and execution belong to the shared runtime.

pub mod esm_imports;
pub mod host;
pub mod modules;
pub mod polyfills;
pub mod shared_run;

pub use host::{NodeHost, NodeOutputSink};
pub use shared_run::EntryGoal;
