> **Live dashboard of test health. Updated by the watchdog as the code changes.**

# Task 353: Test coverage status

## Status: IN PROGRESS

## Summary

The runtime compiles and most tests pass. Two `var_hoisting_tdz.rs` tests still fail, so the hoisting/TDZ milestone is not closed.

## Current results (2026-07-08)

| Test file | Total | Passed | Failed | Ignored |
|---|---|---:|---:|---:|
| `lib.rs` unit tests | 55 | 55 | 0 | 0 |
| `conformance.rs` | 4 | 2 | 0 | 2 |
| `depth_limit.rs` | 9 | 9 | 0 | 0 |
| `equality_operators.rs` | 14 | 14 | 0 | 0 |
| `modules.rs` | 5 | 5 | 0 | 0 |
| `native_extensions.rs` | 8 | 8 | 0 | 0 |
| `project.rs` | 7 | 6 | 0 | 1 |
| `runtime_issues.rs` | 44 | 44 | 0 | 0 |
| `scenarios.rs` | 39 | 39 | 0 | 0 |
| `test262.rs` | 4 | 0 | 0 | 4 |
| `to_primitive.rs` | 10 | 10 | 0 | 0 |
| `var_hoisting_tdz.rs` | 17 | 15 | 2 | 0 |
| **Total** | **208** | **207** | **2** | **7** |

Run:

```bash
cargo test -p quench-runtime
```

## Remaining failures

- `var_hoisting_tdz.rs::test_constructor_returns_this_not_expression_value`
- `var_hoisting_tdz.rs::test_tdz_shadowing_inner_let`

These block Task 292 and any dependent milestone.

## Example status

| Example | Status | Notes |
|---|---|---|
| `examples/counter.js` | ⚠️ Partial | Runs but logs `ReferenceError: Cannot access 'rootId' before initialization` |
| `examples/use-bridge.tsx` | ❌ Fail | `ReferenceError: Cannot access 'props' before initialization` |
| `examples/animations.tsx` | ❌ Fail | `ReferenceError: Cannot access 'props' before initialization` |

## Targets

- **Suite:** `tooling`
- **Batch:** 0
- **Target subset:** n/a (status tracking)
- **Blocked by:** none
- **Exit criteria:** This dashboard accurately reflects the latest `cargo test -p quench-runtime` output.

## Verification

```bash
cargo test -p quench-runtime 2>&1 | tee /tmp/test-output.log
# Update the table above to match the actual counts.
```
