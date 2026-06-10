# Task 007: Bridge: I/O & Exit

## Status
✅ **Done**


## Goal
Implement stdout/stderr writes, raw mode query, and app exit.

## Acceptance Criteria
- [ ] `__ink_stdout_write(data)` writes to terminal via crossterm.
- [ ] `__ink_stderr_write(data)` writes to stderr.
- [ ] `__ink_stdin_is_raw()` → `bool` returns current raw mode state.
- [ ] `__ink_exit()` sets atomic flag to break event loop; optional error code.
- [ ] Unit test: write to stdout/stderr buffers, verify output bytes.

## Dependencies
- Task 001

> ⚠️ **Known issues:**
> - `__ink_stdin_is_raw()` is a stub that always returns `false` (Task 091).
> - `__ink_exit()` uses `process::exit(0)` which bypasses terminal cleanup destructors (Task 074).

## SPEC Reference
§3 Rust Modules (bridge/io.rs)
