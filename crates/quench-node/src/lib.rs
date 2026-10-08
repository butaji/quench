//! `quench-node` is a Node.js-API compatibility host built on top of the
//! shared Quench runtime. The runtime owns language semantics; this crate is the
//! only piece of the workspace allowed to know what
//! "Node" is. Keep the host boundary and runtime semantics separate.
//!
//! Architecture: Node host state, capability dispatch, and observable API
//! handlers are Rust. A small, explicit set of compatibility bridge fragments
//! may assemble those Rust capabilities into Node-shaped objects; they are
//! data evaluated by `quench-runtime`, never a second VM or builtin runtime.
//! The host installs Node builtins through the shared runtime's embedding API.
//!
//! One canonical `NodeSpec` table in `registry` declares every Node
//! global, every `node:` module, and the cap-dispatch ids. A single
//! `NodeHost` lowers that table into the shared runtime.

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
pub mod shared_run;

pub use envelope::{NodeObject, NodeShared};
pub use host::{EntryGoal, NodeHost};
pub use registry::{NodeSpec, NodeSymbol};
