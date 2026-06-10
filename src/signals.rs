//! Signal handling
//!
//! Shared signal state for graceful shutdown on Ctrl+C / SIGTERM.
//! Without this, signals terminate the process without running terminal cleanup.

use std::sync::atomic::{AtomicBool, Ordering};

/// Flag set by SIGINT handler to trigger graceful shutdown
static SHUTDOWN_REQUESTED: AtomicBool = AtomicBool::new(false);

/// Returns true if SIGINT was received and shutdown should begin
pub fn shutdown_requested() -> bool {
    SHUTDOWN_REQUESTED.load(Ordering::SeqCst)
}

/// Register Ctrl+C (SIGINT) and SIGTERM handlers to ensure terminal cleanup.
/// Without this, Ctrl+C terminates the process without running disable_raw_mode().
pub fn setup_signal_handlers() {
    if let Err(e) = ctrlc::set_handler(move || {
        SHUTDOWN_REQUESTED.store(true, Ordering::SeqCst);
    }) {
        tracing::warn!("Could not set Ctrl+C handler: {:?}", e);
    }
}
