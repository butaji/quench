# Task 032: Hook useStdin

## Goal
Implement `useStdin` providing raw mode control and read stream.

## Acceptance Criteria
- [ ] `useStdin()` returns `{isRawModeSupported, setRawMode, stdin}`.
- [ ] `setRawMode(true)` / `setRawMode(false)` toggles crossterm raw mode.
- [ ] `stdin` is a minimal ReadableStream-like object (or just context object).
- [ ] Unit test: toggle raw mode, verify crossterm state changes.

## Dependencies
- Task 031

## SPEC Reference
§5.3 useStdin
