//! Node API host backed by the canonical shared Quench runtime.
//!
//! The host boundary owns Node policy and API behavior. JavaScript semantics
//! and execution belong to the shared runtime.

pub mod esm_imports;
pub mod host;
pub mod modules;
mod node_sources;
pub mod polyfills;
pub mod shared_run;

pub use host::{NodeHost, NodeOutputSink};
pub use shared_run::EntryGoal;

/// On Linux, every binary that links this host (the CLI and the Node test runners alike)
/// allocates through Quench's size-classed small-object allocator, which serves small requests
/// only once the process's memory policy asks for it (`shared_run::apply_allocator_policy`);
/// until then, and under the memory-first policy, every request goes to the system allocator.
#[cfg(target_os = "linux")]
#[global_allocator]
static GLOBAL_ALLOCATOR: quench_alloc::QuenchAlloc = quench_alloc::QuenchAlloc::new();
