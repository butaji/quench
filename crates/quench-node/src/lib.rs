//! `quench-node` is a Node.js-API compatibility host built on top of
//! `quench-runtime`. The runtime is a pure JavaScript engine; this
//! crate is the only piece of the workspace allowed to know what
//! "Node" is. See `docs/adr/0002-quench-node-scope.md` for the
//! scope, the data + patterns + machines + effects shape, and the
//! v1 module set. The ordered plan is in `docs/NODE-STAGES.md`.
//!
//! Architecture: every Node API is a pure Rust object. There is no
//! self-hosted JavaScript builtin layer and no JS bridge. The host
//! installs Node builtins into the runtime through the same public
//! `VmContext` / `host_api` / `execute` API that test262 uses.
//!
//! One canonical `NodeSpec` table in `registry` declares every Node
//! global, every `node:` module, and the cap-dispatch ids. A single
//! `install` function lowers that table into a `VmContext`.

pub mod dispatch;
pub mod dispatch_buffer;
pub mod dispatch_fs;
pub mod dispatch_handlers;
pub mod envelope;
pub mod host;
pub mod modules;
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
