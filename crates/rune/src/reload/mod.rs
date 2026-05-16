//! # Reload Module
//!
//! Manages hot reload of dylibs and host signaling.

mod watcher;
mod host;

pub use watcher::DylibWatcher;
pub use host::HostSignaler;

/// Error type for reload operations.
#[derive(Debug, thiserror::Error)]
pub enum ReloadError {
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    #[error("Library error: {0}")]
    Library(String),

    #[error("Protocol error: {0}")]
    Protocol(String),
}

/// Result type for reload operations.
pub type ReloadResult<T> = Result<T, ReloadError>;
