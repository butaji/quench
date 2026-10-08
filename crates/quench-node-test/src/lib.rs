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

pub mod case_process;
pub mod compat_cli;
pub(crate) mod fixture_metadata;
pub mod inventory;
pub(crate) mod node_observations;
pub mod outcome;
pub mod parallel_profile;
pub mod shared_runner;
pub mod stages;

pub use outcome::NodeOutcome;
pub use stages::{list_stages, resolve_stages, NodeStage, ResolvedStage};
