//! Shared runtime state
//!
//! This module provides a shared thread-local runtime for all bridge modules.

use crate::ink::InkRuntime;
use std::cell::RefCell;

thread_local! {
    /// The shared Ink runtime instance
    pub static INK_RUNTIME: RefCell<InkRuntime> = RefCell::new(InkRuntime::new());
}

/// Reset the runtime to a fresh state
pub fn reset_runtime() {
    INK_RUNTIME.with(|runtime| {
        *runtime.borrow_mut() = InkRuntime::new();
    });
}
