# Task 286: Remove redundant Context::globals HashMap

## Status: PENDING

## Problem

`Context` maintains both `self.globals` and `self.env`, inserting globals into both maps. This duplicates storage and adds extra clones.

## Fix

Remove `Context::globals`; use the top-level `Environment` scope as the global object.

## Acceptance criteria

- [ ] `Context::globals` field removed.
- [ ] `set_global`/`get_global` operate on the top-level environment.
- [ ] All existing global-access tests pass.
- [ ] JS scenario test for global variable read/write.

## Files

- `crates/quench-runtime/src/lib.rs`

## Verification

```bash
cargo test -p quench-runtime globals_via_env
cargo test -p quench-runtime scenarios::global
```
