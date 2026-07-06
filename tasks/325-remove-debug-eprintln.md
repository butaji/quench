> New low-effort/high-impact win from code review.

# Task 325: Remove debug eprintln! spam

## Status: PENDING

## Problem

The TDZ refactor left dozens of `eprintln!` debug statements on every variable declaration, scope push/pop, and TDZ check. This pollutes test output, slows the interpreter, and makes conformance logs unusable.

## Fix

Remove all debug prints from `env.rs` and `interpreter.rs`.

## Acceptance criteria

- [ ] No `eprintln!` calls remain in `env.rs` or `interpreter.rs`.
- [ ] Test output is clean.
- [ ] Regression test added (or existing tests continue to pass).

## Files

- `crates/quench-runtime/src/env.rs`
- `crates/quench-runtime/src/interpreter.rs`

## Effort / impact

- Effort: trivial
- Impact: medium
