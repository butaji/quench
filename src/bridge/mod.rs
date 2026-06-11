//! Bridge module
//!
//! Splits the monolithic bridge.rs into focused submodules:
//! - node: Node creation
//! - tree: Tree mutations
//! - timers: Timer/microtask system
//! - io: I/O functions
//! - ffi: FFI dispatch
//! - props: JSON props parsing

pub mod ffi;
pub mod io;
pub mod node;
pub mod props;
pub mod timers;
pub mod tree;

// Re-export all functions for backwards compatibility
pub use node::{
    __ink_create_node, __ink_create_root, __ink_create_text_node, __ink_destroy_root, __ink_render_element,
};
pub use tree::{
    __ink_append_child, __ink_calculate_layout, __ink_clear_dirty, __ink_commit,
    __ink_commit_update, __ink_get_layout, __ink_get_node_children, __ink_get_node_parent,
    __ink_get_node_prop, __ink_get_node_prop_raw, __ink_get_node_props_json, __ink_get_node_tag,
    __ink_get_node_text, __ink_get_root_id, __ink_insert_before, __ink_is_dirty, __ink_remove_child,
    __ink_set_text,
};
pub use timers::{
    __ink_clear_timer, __ink_drain_microtasks, __ink_enqueue_microtask, __ink_has_pending_timers,
    __ink_next_timer_delay, __ink_process_timers, __ink_set_interval, __ink_set_timeout,
};
pub use io::{
    __ink_exit, __ink_get_exit_code, __ink_get_terminal_size, __ink_measure_text, __ink_reset_exit,
    __ink_set_exit_requested, __ink_set_terminal_size, __ink_should_exit, __ink_stderr_write,
    __ink_stdin_is_raw, __ink_stdout_write,
};
pub use ffi::{call_ink_ffi, call_ink_ffi_fast, get_fast_method_id, FastMethodId};

/// Bridge errors
#[derive(thiserror::Error, Debug)]
pub enum FfiError {
    #[error("Invalid argument: {0}")]
    InvalidArgument(String),

    #[error("Bridge call failed: {0}")]
    CallFailed(String),

    #[error("Ink error: {0}")]
    InkError(#[from] crate::ink::InkError),
}

pub type Result<T> = std::result::Result<T, FfiError>;
