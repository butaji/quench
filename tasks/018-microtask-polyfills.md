# Task 018: Microtask Polyfills

## Goal
Implement `setImmediate` / `clearImmediate` and `process.nextTick` via microtask queue.

## Acceptance Criteria
- [ ] `setImmediate(cb)` → `u32` queues callback on next tick of event loop.
- [ ] `process.nextTick(cb)` identical behavior.
- [ ] Microtasks execute before timers and I/O in each loop iteration.
- [ ] Unit test: queue microtask + timer; verify microtask runs first.

> ⚠️ **NOT STARTED**: `setImmediate` and `process.nextTick` are not implemented.

## Dependencies
- Task 013

## SPEC Reference
§2 Polyfills — setImmediate / process.nextTick
