//! `quench-node-test` owns the Node.js test runner: discovery,
//! composition, execution, and completion classification.
//!
//! This crate depends on `quench-node` (the host). It is
//! forbidden from modifying the upstream fixture tree, from
//! rewriting Node harness behavior, and from designing the
//! Node API surface. The host is forbidden from knowing about
//! this crate, the runner, the fixtures, or Node test policy.
//!
//! Keep runner policy separate from the Node host and runtime semantics.

pub mod reader;
pub mod runner;
pub mod stages;

pub use reader::{NodeFixture, NodeOutcome, NodeRunner};
pub use runner::{run_file, NodeTestRunner};
pub use stages::{list_stages, resolve_stages, NodeStage, ResolvedStage};
