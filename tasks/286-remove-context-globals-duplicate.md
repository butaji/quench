# Task 286: Remove redundant Context::globals HashMap

## Status: COMPLETED

## Problem

`Context` maintained both `self.globals` and `self.env`, inserting globals into both maps. This duplicated storage and added extra clones.

## Fix

Removed the `globals` field from `Context`. `set_global`/`get_global` now operate exclusively on the top-level `Environment` scope.

## Acceptance criteria

- [x] `Context::globals` field removed.
- [x] `set_global`/`get_global` operate on the top-level environment.
- [x] All existing global-access tests pass.
- [x] JS scenario test for global variable read/write (`scenario_global_read_write`).

## Files

- `crates/quench-runtime/src/lib.rs`
- `crates/quench-runtime/tests/scenarios.rs`

## Verification

```bash
cargo test -p quench-runtime scenario_global_read_write
cargo test -p quench-runtime --lib tests::test_globals
cargo test -p quench-runtime
```

All pass.

## Targets

- **Suite:** `both`
- **Batch:** 1
- **Target subset:** n/a (runtime cleanup)
- **Blocked by:** none
- **Exit criteria:** `Context::globals` removed; global reads/writes use the top-level environment.
