//! Ink module
//!
//! Splits the monolithic ink.rs into focused submodules:
//! - node: InkNode struct and prop application
//! - tree: Tree operations
//! - runtime: InkRuntime state management
//! - shared: Shared thread-local runtime

pub mod node;
pub mod runtime;
pub mod shared;
pub mod tree;

// Re-export public types
pub use node::{InkNode, InkTag, PropValue};
pub use runtime::InkRuntime;
pub use tree::{append_child, remove_child, insert_before, commit_update, set_text};
#[allow(unused_imports)]
pub use shared::{INK_RUNTIME, reset_runtime};

// Re-export errors
pub use runtime::InkError;
