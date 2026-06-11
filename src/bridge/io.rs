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

/// Request exit (used for Ctrl+C handling)
pub fn __ink_set_exit_requested() {
    SHOULD_EXIT.store(true, Ordering::SeqCst);
    EXIT_CODE.store(130, Ordering::SeqCst); // 128 + 2 = SIGINT
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

    // Strip ANSI escape sequences so they aren't counted as visible characters.
    // Without this, colored/styled text is measured wider than it renders.
    let clean_text = strip_ansi(text);

    let max_width = if max_width == 0 { 80 } else { max_width as usize };
    let lines = wrap(&clean_text, max_width);

    let width = lines
        .iter()
        .map(|l| UnicodeWidthStr::width(l.as_ref()))
        .max()
        .unwrap_or(0) as u32;

    let height = lines.len() as u32;

    (width, height)
}

/// Strip ANSI escape sequences from text (ESC["\x30-\x7e"]*["\x40-\x7e"])
fn strip_ansi(s: &str) -> String {
    let bytes = s.as_bytes();
    let mut result = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == 0x1B && i + 1 < bytes.len() && bytes[i + 1] == b'[' {
            // Skip until we hit a final byte in range 0x40-0x7e
            i += 2;
            while i < bytes.len() && (bytes[i] < 0x40 || bytes[i] > 0x7e) {
                i += 1;
            }
            if i < bytes.len() {
                i += 1; // consume the final byte
            }
        } else {
            result.push(bytes[i]);
            i += 1;
        }
    }
    String::from_utf8_lossy(&result).into_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_strip_ansi_basic() {
        // No ANSI codes - unchanged
        assert_eq!(strip_ansi("hello"), "hello");
        assert_eq!(strip_ansi(""), "");
    }

    #[test]
    fn test_strip_ansi_colors() {
        // ANSI color codes should be stripped
        assert_eq!(strip_ansi("\x1b[31mred\x1b[0m"), "red");
        assert_eq!(strip_ansi("\x1b[1;32mgreen\x1b[0m"), "green");
        assert_eq!(strip_ansi("\x1b[38;5;196mmagenta\x1b[0m"), "magenta");
    }

    #[test]
    fn test_strip_ansi_bold_dim() {
        // Bold and dim codes
        assert_eq!(strip_ansi("\x1b[1mbold\x1b[0m"), "bold");
        assert_eq!(strip_ansi("\x1b[2mdim\x1b[0m"), "dim");
    }

    #[test]
    fn test_measure_text_with_ansi() {
        // ANSI codes must NOT be counted as visible width
        let (w, h) = __ink_measure_text("\x1b[31mred\x1b[0m", 80);
        assert_eq!(w, 3, "ANSI codes should not affect visible width");
        assert_eq!(h, 1);
    }

    #[test]
    fn test_measure_text_styled_hello() {
        let (w, h) = __ink_measure_text("\x1b[1;37mHello\x1b[0m", 80);
        assert_eq!(w, 5);
        assert_eq!(h, 1);
    }
}
