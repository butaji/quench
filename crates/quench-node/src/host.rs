//! Canonical Node host backed by the shared runtime.

pub(crate) mod compile_cache;
pub(crate) mod node_host;
pub(crate) mod real_path;
pub mod resident_pages;
pub(crate) mod shared_vm;
pub(crate) mod startup_snapshot;

pub(crate) use crate::shared_run::EntryGoal;
pub(crate) use node_host::SharedNodeState;
pub use node_host::{NodeHost, NodeOutputSink};
