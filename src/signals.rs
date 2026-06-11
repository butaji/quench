//! Signal handling
//!
//! Shared signal state for graceful shutdown on Ctrl+C / SIGTERM.
//! Without this, signals terminate the process without running terminal cleanup.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;

/// Flag set by SIGINT handler to trigger graceful shutdown
static SHUTDOWN_REQUESTED: AtomicBool = AtomicBool::new(false);

/// Terminal modes that have been explicitly enabled.
/// Cleanup only disables the modes that were actually turned on.
#[derive(Debug, Clone, Default)]
pub struct TerminalModes {
    pub raw_mode: bool,
    pub alt_screen: bool,
    pub mouse_tracking: bool,
    pub bracketed_paste: bool,
}

static TERMINAL_MODES: Mutex<TerminalModes> = Mutex::new(TerminalModes {
    raw_mode: false,
    alt_screen: false,
    mouse_tracking: false,
    bracketed_paste: false,
});

/// Record that a terminal mode has been enabled.
pub fn set_terminal_mode(mode: &str, enabled: bool) {
    if let Ok(mut modes) = TERMINAL_MODES.lock() {
        match mode {
            "raw" => modes.raw_mode = enabled,
            "alt_screen" => modes.alt_screen = enabled,
            "mouse" => modes.mouse_tracking = enabled,
            "bracketed_paste" => modes.bracketed_paste = enabled,
            _ => {}
        }
    }
}

/// Get a snapshot of currently tracked terminal modes.
pub fn get_terminal_modes() -> TerminalModes {
    TERMINAL_MODES.lock().map(|m| m.clone()).unwrap_or_default()
}

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

/// Reset the terminal to a sane state.  Safe to call from a signal handler
/// or a panic hook — it only does the minimum needed to put the terminal
/// back into a usable mode (raw off, cursor visible, screen restored, mouse
/// capture off, bracketed paste off).
pub fn reset_terminal_state() {
    use std::io::Write;
    let modes = get_terminal_modes();
    let mut out = std::io::stdout();

    // Best-effort: ignore errors.  Many of these are no-ops in some terminals.
    if modes.raw_mode {
        let _ = crossterm::terminal::disable_raw_mode();
    }
    let _ = out.write_all(b"\x1b[?25h");      // show cursor
    if modes.mouse_tracking {
        let _ = out.write_all(b"\x1b[?1000l");     // disable mouse tracking
        let _ = out.write_all(b"\x1b[?1002l");     // disable mouse drag tracking
        let _ = out.write_all(b"\x1b[?1003l");     // disable mouse move tracking
        let _ = out.write_all(b"\x1b[?1006l");     // disable SGR mouse mode
    }
    if modes.alt_screen {
        let _ = out.write_all(b"\x1b[?1049l");     // leave alternate screen
    }
    if modes.bracketed_paste {
        let _ = out.write_all(b"\x1b[?2004l");     // disable bracketed paste
    }
    let _ = out.flush();
}

/// Install a panic hook that resets the terminal *before* the standard panic
/// message is printed.  This prevents a panic inside the TUI loop from
/// leaving the user's terminal stuck in raw mode with the cursor hidden.
pub fn install_panic_cleanup() {
    let prev = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        reset_terminal_state();
        prev(info);
    }));
}
