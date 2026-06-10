# Task 034: Hook useStderr

## Status
✅ **Done**


## Goal
Implement `useStderr` providing write access to stderr.

## Acceptance Criteria
- [ ] `useStderr()` returns `{write}`.
- [ ] `write(data)` delegates to `__ink_stderr_write`.
- [ ] Unit test: write to stderr, verify bytes in stderr buffer.

## Dependencies
- Task 031

## SPEC Reference
§4 JS Runtime (runtime.js hooks)
