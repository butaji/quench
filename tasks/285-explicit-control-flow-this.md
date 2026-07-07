# Task 285: Make control flow and this explicit

## Status: PENDING

## Problem

`break`/`continue` are propagated via a thread-local flag (`CONTROL_FLOW`), and native `this` is stashed in a thread-local (`CURRENT_THIS`). This breaks composability and conflicts with the HIR design.

## Fix

- Return control flow via a `ControlFlow` enum inside `Result`.
- Pass `this` as an explicit argument through `call_value_with_this` and related helpers.
- Remove the thread-local storage.

## Acceptance criteria

- [ ] `CONTROL_FLOW` thread-local removed.
- [ ] `CURRENT_THIS` thread-local removed.
- [ ] All tests for break/continue/return and method calls still pass.
- [ ] JS scenario tests for nested control flow and method calls.

## Files

- `crates/quench-runtime/src/interpreter.rs`

## Verification

```bash
cargo test -p quench-runtime control_flow_explicit
cargo test -p quench-runtime scenarios::control_flow
```

## Targets

- **Suite:** `runtime`
- **Batch:** 6
- **Target subset:** n/a (interpreter cleanup)
- **Blocked by:** 85
- **Exit criteria:** `CONTROL_FLOW` and `CURRENT_THIS` thread-locals removed; control flow and `this` are explicit.
