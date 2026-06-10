//! Bridge: I/O functions
//!
//! Functions for stdout, stderr, stdin, and exit handling.

use std::io::Write;
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use crate::ink::INK_RUNTIME;

// ===================================================================
// Exit handling
// ===================================================================

/// Exit flag
static SHOULD_EXIT: AtomicBool = AtomicBool::new(false);

/// Exit code
static EXIT_CODE: AtomicU32 = AtomicU32::new(0);

/// Exit the application with optional error code
pub fn __ink_exit(code: i32) {
    SHOULD_EXIT.store(true, Ordering::SeqCst);
    EXIT_CODE.store(code as u32, Ordering::SeqCst);
}

/// Check if exit was requested
pub fn __ink_should_exit() -> bool {
    SHOULD_EXIT.load(Ordering::SeqCst)
}

/// Get exit code
pub fn __ink_get_exit_code() -> u32 {
    EXIT_CODE.load(Ordering::SeqCst)
}

/// Reset exit state (for reuse in tests)
pub fn __ink_reset_exit() {
    SHOULD_EXIT.store(false, Ordering::SeqCst);
    EXIT_CODE.store(0, Ordering::SeqCst);
}

// ===================================================================
// I/O
// ===================================================================

/// Write to stdout
pub fn __ink_stdout_write(data: &str) {
    let _ = std::io::stdout().write_all(data.as_bytes());
}

/// Write to stderr
pub fn __ink_stderr_write(data: &str) {
    eprint!("{}", data);
}

/// Check if stdin is in raw mode
pub fn __ink_stdin_is_raw() -> bool {
    false
}

/// Set raw mode on stdin
pub fn __ink_set_raw_mode(_enabled: bool) {}

// ===================================================================
// Terminal state
// ===================================================================

/// Set terminal dimensions
pub fn __ink_set_terminal_size(width: u32, height: u32) {
    INK_RUNTIME.with(|runtime| {
        let mut r = runtime.borrow_mut();
        r.set_terminal_size(width, height)
    })
}

/// Get terminal dimensions
pub fn __ink_get_terminal_size() -> (u32, u32) {
    INK_RUNTIME.with(|runtime| runtime.borrow().terminal_size())
}

// ===================================================================
// Text measurement
// ===================================================================

/// Measure text dimensions
pub fn __ink_measure_text(text: &str, max_width: u32) -> (u32, u32) {
    use textwrap::wrap;
    use unicode_width::UnicodeWidthStr;

    if text.is_empty() {
        return (0, 0);
    }

    let max_width = if max_width == 0 { 80 } else { max_width as usize };
    let lines = wrap(text, max_width);

    let width = lines
        .iter()
        .map(|l| UnicodeWidthStr::width(l.as_ref()))
        .max()
        .unwrap_or(0) as u32;

    let height = lines.len() as u32;

    (width, height)
}
