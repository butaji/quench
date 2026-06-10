# Task 031: Hook useApp

## Status
✅ **Done**


## Goal
Implement `useApp` returning exit, stdout, stdin, stderr proxies.

## Acceptance Criteria
- [ ] `useApp()` returns `{exit, stdout, stdin, stderr}`.
- [ ] `exit(err?)` calls `__ink_exit()`.
- [ ] `stdout.write(data)` calls `__ink_stdout_write(data)`.
- [ ] `stdin.isRawModeSupported()` returns `__ink_stdin_is_raw()`.

> ⚠️ **Known issue:** `__ink_stdin_is_raw()` is a stub that always returns `false`. See Task 091.
- [ ] Unit test: mock bridge globals, verify each method calls correct function.

## Dependencies
- Task 012, Task 007

## SPEC Reference
§4 JS Runtime (runtime.js hooks)
