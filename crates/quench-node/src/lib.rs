//! Node API host backed by the canonical shared Quench runtime.
//!
//! The host boundary owns Node policy and API behavior. JavaScript semantics
//! and execution belong to the shared runtime.

#![cfg_attr(all(test, feature = "bench-primitives"), feature(test))]

#[cfg(feature = "native-execution")]
pub mod aot;
pub mod esm_imports;
pub mod host;
pub mod modules;
pub mod shared_run;

pub use host::{NodeHost, NodeOutputSink};

/// The environment variable naming the file a `quench-node-test` case process writes its result to.
pub const CASE_RESULT_ENV: &str = "QUENCH_NODE_TEST_CASE_RESULT";
pub use shared_run::EntryGoal;

/// On unix, every binary that links this host (the CLI and the Node test runners alike)
/// allocates through Quench's size-classed small-object allocator, which serves small requests
/// only once the process's memory policy asks for it (`shared_run::apply_allocator_policy`);
/// until then, and under the memory-first policy, every request goes to the system allocator.
#[cfg(unix)]
#[global_allocator]
static GLOBAL_ALLOCATOR: quench_alloc::QuenchAlloc = quench_alloc::QuenchAlloc::new();

#[cfg(feature = "native-execution")]
pub const NATIVE_SUPPORT_IS_EXECUTION_ONLY: bool = !cfg!(any(feature = "jit", feature = "aot", feature = "bundled-sources"));
