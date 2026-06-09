# Task 030: Hook useInput

## Goal
Full `useInput` implementation with options and keyboard dispatch.

## Acceptance Criteria
- [ ] `useInput(handler, {isActive})` registers on mount, unregisters on unmount.
- [ ] When `isActive === false`, handler is not invoked.
- [ ] Handler receives `(input, key)` where `key` has `name`, `ctrl`, `shift`, `meta`.
- [ ] Multiple `useInput` hooks in same app all receive events.
- [ ] Example test: Counter app with `useInput` responds to space/q keys.

## Dependencies
- Task 012, Task 014

## SPEC Reference
§5.3 useInput
