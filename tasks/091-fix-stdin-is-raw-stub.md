# Task 091: Fix stdin_is_raw Hardcoded Stub

## Status: 🟡 **MODERATE — NOT STARTED**

## Goal
Implement `__ink_stdin_is_raw()` to return the actual raw mode state instead of hardcoded `false`.

## Problem

`src/bridge/io.rs::__ink_stdin_is_raw()` always returns `false`:

```rust
pub fn __ink_stdin_is_raw() -> bool {
    false  // Always false, even when raw mode IS enabled
}
```

Hooks like `useStdin` that check `isRawModeSupported` or `isRawMode` get incorrect values. Apps that conditionally behave based on raw mode state will malfunction.

## Fix

Track raw mode state in the bridge:

```rust
static IS_RAW_MODE: AtomicBool = AtomicBool::new(false);

pub fn __ink_set_raw_mode(enabled: bool) {
    IS_RAW_MODE.store(enabled, Ordering::SeqCst);
    if enabled {
        let _ = crossterm::terminal::enable_raw_mode();
    } else {
        let _ = crossterm::terminal::disable_raw_mode();
    }
}

pub fn __ink_stdin_is_raw() -> bool {
    IS_RAW_MODE.load(Ordering::SeqCst)
}
```

Also set `IS_RAW_MODE = true` when `setup_terminal()` enables raw mode in `main.rs`.

## Acceptance Criteria
- [ ] `__ink_stdin_is_raw()` returns `true` when raw mode is enabled
- [ ] `__ink_stdin_is_raw()` returns `false` when raw mode is disabled
- [ ] `useStdin` hook reports correct raw mode status
- [ ] Test: enable raw mode, verify `stdin_is_raw()` returns true

## Files to Modify
- `src/bridge/io.rs` — Track and report raw mode state
- `src/main.rs` — Set flag when enabling raw mode

## References
- crossterm raw mode: https://docs.rs/crossterm/latest/crossterm/terminal/
