# Task 032: Hook useStdin

## Status
✅ **Done**


## Goal
Implement `useStdin` providing raw mode control and read stream.

## Acceptance Criteria
- [ ] `useStdin()` returns `{isRawModeSupported, setRawMode, stdin}`.
- [ ] `setRawMode(true)` / `setRawMode(false)` toggles crossterm raw mode via `__ink_set_raw_mode`.
- [ ] `stdin` is a minimal ReadableStream-like object (or just context object).
- [ ] Unit test: toggle raw mode, verify crossterm state changes.

## Dependencies
- Task 031

> ⚠️ **Known issue:** `__ink_stdin_is_raw()` is a stub that always returns `false`, so `useStdin`'s `isRawModeSupported` and `isRawMode` values may be incorrect. See Task 091.

## SPEC Reference
§4 JS Runtime (runtime.js hooks)
