//! Node host for the single shared OXC/Wasm runtime.

#[path = "host_shared.rs"]
pub mod host;
#[path = "modules/shared_only.rs"]
pub mod modules;
pub mod polyfills;
pub mod shared_run;

pub use host::NodeHost;
