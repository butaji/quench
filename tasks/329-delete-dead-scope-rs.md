> New low-effort/low-impact cleanup from code review.

# Task 329: Delete dead scope.rs module

## Status: PENDING

## Problem

`crates/quench-runtime/src/scope.rs` contains a complete duplicate `Scope`/`VarState` implementation that is never referenced (not exported from `lib.rs`, no `crate::scope` usages). It confuses the codebase.

## Fix

Delete the file.

## Acceptance criteria

- [ ] `scope.rs` removed.
- [ ] `cargo check` still passes.

## Files

- `crates/quench-runtime/src/scope.rs`

## Effort / impact

- Effort: trivial
- Impact: low
