# Quench Runtime - Execution Status

## Current state (2026-07-08)

### Test results

| Suite / file | Total | Passed | Failed | Status |
|--------------|-------|--------|--------|--------|
| `lib.rs` unit tests | 55 | 55 | 0 | ✅ |
| `runtime_issues.rs` (parallel) | 44 | 44 | 0 | ✅ |
| `scenarios.rs` | 39 | 39 | 0 | ✅ |
| `var_hoisting_tdz.rs` | 17 | 15 | 2 | ❌ |
| Other integration tests | ~50 | ~50 | 0 | ✅ |

Run with `cargo test -p quench-runtime`.

### Example results

| Example | Status | Notes |
|---------|--------|-------|
| `examples/simple.js` | ✅ | Passes |
| `examples/counter.js` | ⚠️ | Runs but logs `ReferenceError: Cannot access 'rootId' before initialization` |
| `examples/use-bridge.tsx` | ❌ | `ReferenceError: Cannot access 'props' before initialization` |
| `examples/animations.tsx` | ❌ | `ReferenceError: Cannot access 'props' before initialization` |

## Diagnosed issues and exact fixes

### 1. False "Maximum call stack size exceeded" in parallel tests

**Symptom:** Simple built-in calls failed with `Maximum call stack size exceeded` when tests ran in parallel.

**Root cause:** `CURRENT_DEPTH` was a global `static AtomicUsize`; concurrent threads accumulated each other's recursion counts.

**Status:** Fixed by moving to a thread-local depth counter (Task 338). Parallel `runtime_issues.rs` now passes 44/44.

**Tracking:** Task 338.

### 2. Recursive interpreter consumes the native Rust stack

**Symptom:** Deeply recursive JS functions exhaust the native Rust stack.

**Root cause:** Each JS function call translates to multiple nested Rust calls.

**Exact fix (by design):** Replace recursion with a non-recursive state machine / trampoline loop over a heap-allocated `Vec<CallFrame>` (Task 85).

**Current state:** Examples no longer crash with stack overflow, but they still fail with `ReferenceError` due to incomplete hoisting/TDZ.

**Tracking:** Task 85, Task 354.

### 3. `var` hoisting / `let` / `const` TDZ

**Symptom:** Function-scope `var` was not hoisted and TDZ was not enforced.

**Status:** Partially fixed (Tasks 339, 340). `var_hoisting_tdz.rs` passes 15/17; 2 tests still fail:
- `test_constructor_returns_this_not_expression_value`
- `test_tdz_shadowing_inner_let`

**Tracking:** Task 292, Task 339, Task 340.

## Exit criteria for this status page

- [ ] `var_hoisting_tdz.rs` passes 17/17 in parallel.
- [ ] `examples/use-bridge.tsx` and `examples/animations.tsx` run without initialization errors.
- [ ] Task 85 (trampoline interpreter) lands.
