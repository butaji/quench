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
#[cfg(feature = "native-execution")]
pub use node_host::NativeProgramResolver;

/// A built-in whose implementation was JavaScript embedded in quench. Quench implements
/// Node in Rust only (decision 38); until this part has a Rust port, building it fails.
pub(crate) fn removed_javascript(origin: impl AsRef<str>) -> Result<quench_runtime::RootId, quench_runtime::RootedError> {
    Err(quench_runtime::RootedError::host(format!(
        "{}: not implemented in Rust",
        origin.as_ref()
    )))
}
