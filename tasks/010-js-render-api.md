# Task 010: JS render() API

## Goal
Implement the `render()` entry point matching Ink's API.

## Acceptance Criteria
- [ ] `render(node, options)` creates container via reconciler, calls `__ink_create_root`.
- [ ] Returns `{waitUntilExit, unmount}`.
- [ ] `unmount()` destroys React container and calls `__ink_destroy_root`.
- [ ] `waitUntilExit()` resolves on `__ink_exit`.
- [ ] Integration test: render + unmount cycle completes without panic.

## Dependencies
- Task 009, Task 002

## SPEC Reference
§5.1 `render(<App />, {stdout, stdin, stderr, debug, patchConsole})`
