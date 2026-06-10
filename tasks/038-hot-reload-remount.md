# Task 038: DevEx: Remount Cycle

## Status: ⚠️ **PARTIALLY IMPLEMENTED — BUG DISCOVERED (Task 072)**

## Goal
Implement fast unmount/eval/remount for hot reload without VM restart.

## Current Implementation

Hot reload is wired up in `src/event_loop.rs` but **creates a new rquickjs Runtime/Context** instead of reusing the existing one. The new context never gets `setup_runtime()` called on it, so `__ink_call`, `runtime.js`, hooks, and bridge config are all missing. The reloaded script runs in a bare VM and silently fails.

See **Task 072** for the full bug description and fix plan.

## Acceptance Criteria
- [x] File watcher (`notify`) integrated via `hotreload.rs`
- [x] `--watch` and `--hot` CLI flags exist
- [ ] `ctx.eval(new_bundle)` loads updated code in **same** rquickjs runtime (currently creates new VM)
- [ ] JS `unmount()` callback destroys React root and calls `__ink_destroy_root`
- [ ] Total reload latency < 50 ms measured end-to-end
- [ ] Integration test: modify text in example, reload, verify new text rendered

## Dependencies
- Task 010, Task 037, Task 002, Task 072

## SPEC Reference
§11 Post-Review Remediation (hot reload sub-topic)
