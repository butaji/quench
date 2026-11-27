//! The layer crates of the Quench runtime, linked with its VM core. `quench-runtime` is the VM
//! core; each layer crate adds behaviour above it (`quench_runtime::vm::layer_hooks`), and the
//! core calls into a layer through symbols the layer defines. A binary that uses the runtime
//! depends on this crate and names it (`extern crate quench_layers;`) so every layer is linked.
#[cfg(feature = "intl")]
extern crate quench_runtime_intl;
#[cfg(not(feature = "intl"))]
mod intl_stubs;
extern crate quench_runtime_engine;
#[cfg(feature = "jit")]
extern crate quench_stencils;

/// The engine layer's public surface (`quench_runtime_engine::api`).
pub use quench_runtime_engine::api as engine;
