//! Canonical Node host backed by the shared runtime.

pub(crate) mod compile_cache;
pub(crate) mod hmr;
pub(crate) mod node_host;
pub(crate) mod real_path;
pub mod process_start;
pub mod resident_pages;
pub(crate) mod shared_vm;
pub(crate) mod ts_checker;
pub(crate) mod startup_snapshot;

pub(crate) use crate::shared_run::EntryGoal;
pub(crate) use node_host::{SharedNodeState, Teardown};
pub use node_host::{unlinked_operation, NodeHost, NodeOutputSink};
pub use shared_vm::{OPERATION_CALLBACKS, OPERATION_CALLBACK_PATHS};
#[cfg(feature = "native-execution")]
pub use node_host::NativeProgramResolver;

