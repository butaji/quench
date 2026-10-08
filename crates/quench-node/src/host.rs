//! Canonical Node host backed by the shared runtime.

pub(crate) mod node_host;
pub(crate) mod shared_vm;

pub(crate) use crate::shared_run::EntryGoal;
pub(crate) use node_host::SharedNodeState;
pub use node_host::{NodeHost, NodeOutputSink};
