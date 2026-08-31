//! `quench-node` is a Node.js-API compatibility host built on top of
//! `quench-runtime`. The runtime owns language semantics; this crate is the
//! only piece of the workspace allowed to know what
//! "Node" is. Keep the host boundary and runtime semantics separate.
//!
//! Architecture: Node host state, capability dispatch, and observable API
//! handlers are Rust. A small, explicit set of compatibility bridge fragments
//! may assemble those Rust capabilities into Node-shaped objects; they are
//! data evaluated by `quench-runtime`, never a second VM or builtin runtime.
//! The host installs Node builtins through the same `VmContext` / `host_api` /
//! `execute` boundary used by test262.
//!
//! One canonical `NodeSpec` table in `registry` declares every Node
//! global, every `node:` module, and the cap-dispatch ids. A single
//! `install` function lowers that table into a `VmContext`.

pub mod dispatch;
pub mod dispatch_buffer;
pub mod dispatch_fs;
pub mod dispatch_handlers;
pub mod envelope;
pub mod esm_imports;
pub mod host;
pub mod modules;
pub mod polyfills;
pub mod registry;
pub mod run;

pub use envelope::{NodeObject, NodeShared};
pub use host::{install, NodeHost};
pub use registry::{NodeSpec, NodeSymbol};

use quench_runtime::value::Value;

/// Canonical Node API surface entry. Returned by `install`.
pub struct NodeRealm {
    pub node_value: Value,
    pub process_value: Value,
    pub console_value: Value,
    pub buffer_value: Value,
    pub timers_value: Value,
    pub global_value: Value,
}
