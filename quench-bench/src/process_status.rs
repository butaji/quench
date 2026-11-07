//! Process exit facts shared by the measurement binaries (`#[path]`-included).

/// The exit status a shell reports for a process a signal ended, before the signal number.
pub const SIGNAL_STATUS_BASE: i32 = 128;
