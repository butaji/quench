# Task 012: JS Hook Shims

## Goal
Implement thin JS wrappers for `useInput` and `useApp` that bridge to.

## Acceptance Criteria
- [ ] `useInput(handler, options)` registers/unregisters via `__ink_register_input` in `useEffect`.
- [ ] `useInput` respects `options.isActive`.
- [ ] `useApp()` returns object with `exit`, `stdout`, `stdin`, `stderr` using `__ink_*`.
- [ ] Unit test: mock `globalThis.__ink_*`, verify registration and cleanup.

## Dependencies
- Task 008, Task 007

## SPEC Reference
§5.3 Hooks
