//! # Hot Reload Module
//!
//! Provides hot reload functionality for dylib-based development.

mod watcher;
mod host;

pub use watcher::{DylibWatcher, ReloadEvent};
pub use host::HostSignaler;

use thiserror::Error;

/// Result type for reload operations.
pub type ReloadResult<T> = Result<T, ReloadError>;

/// Errors that can occur during hot reload.
#[derive(Error, Debug)]
pub enum ReloadError {
    #[error("Library error: {0}")]
    Library(String),

    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    #[error("Serialization error: {0}")]
    Serialization(String),
}
