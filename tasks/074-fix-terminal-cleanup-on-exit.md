# Task 074: Fix Terminal Cleanup on Exit

## Status: 🔴 **CRITICAL BUG — NOT STARTED**

## Goal
Guarantee terminal restoration (disable raw mode, show cursor) even when panics or errors occur.

## Problem

`src/main.rs::setup_terminal()` ends with:

```rust
let _ = crossterm::terminal::disable_raw_mode();
let _ = terminal.show_cursor();
std::process::exit(0);  // Bypasses ALL destructors
```

If `disable_raw_mode()` or `show_cursor()` fails (or if a panic occurs earlier), `process::exit()` prevents Rust's drop glue from running. The user's terminal stays in raw mode with a hidden cursor — effectively bricked until they run `reset`.

This is especially bad because:
- `let _ =` discards any error from `disable_raw_mode()` — we don't know if it worked
- `process::exit(0)` was added to bypass rquickjs GC assertion failures, but it destroys the cleanup guarantee

## Fix Approaches

### Option A: Scope Guard (Recommended)
Use a RAII guard that restores the terminal on drop, placed early in `setup_terminal()`:

```rust
struct TerminalGuard;
impl Drop for TerminalGuard {
    fn drop(&mut self) {
        let _ = crossterm::terminal::disable_raw_mode();
        let _ = crossterm::execute!(
            std::io::stdout(),
            crossterm::cursor::Show
        );
    }
}

fn setup_terminal(...) -> Result<()> {
    crossterm::terminal::enable_raw_mode()?;
    let _guard = TerminalGuard;  // Will run even on panic or early return
    // ... event loop ...
    // Guard drops here, restoring terminal
    Ok(())
}
```

### Option B: std::panic::catch_unwind
Wrap the event loop in `catch_unwind`, restore terminal in `finally`, then re-panic if needed.

### Option C: Remove process::exit
Instead of `process::exit(0)`, let the function return normally. Address the rquickjs GC assertion through other means (e.g., drop the runtime explicitly before returning).

**Recommendation:** Implement Option A (scope guard) AND Option C (remove `process::exit`). The guard handles panics and early returns; removing `exit` lets normal cleanup proceed.

## Acceptance Criteria
- [ ] `setup_terminal()` uses a RAII guard to guarantee `disable_raw_mode()` runs
- [ ] `setup_terminal()` uses a RAII guard to guarantee cursor is shown
- [ ] `std::process::exit(0)` is removed from `setup_terminal()`
- [ ] Panic during event loop still restores terminal (test with `panic!("test")` injected)
- [ ] Early return from `setup_terminal()` (e.g., raw mode failure) still shows cursor if it was hidden
- [ ] The rquickjs GC assertion is handled without `process::exit` (e.g., explicit runtime drop)

## Files to Modify
- `src/main.rs` — Remove `process::exit(0)`, add `TerminalGuard` RAII struct

## References
- crossterm docs: https://docs.rs/crossterm/latest/crossterm/
- Rust RAII patterns: https://doc.rust-lang.org/rust-by-example/scope/lifetime.html
