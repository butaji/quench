# Task 281: Remove the while-loop 10-iteration cap

## Status: PENDING

## Problem

`interpreter.rs` aborts every `while` loop after 10 iterations. This is a correctness bug, not a safety guard.

## Fix

Remove the arbitrary cap. If infinite-loop protection is desired, implement a JS instruction/branch budget or a much larger configurable limit.

## Acceptance criteria

- [ ] `while` loops run for the actual condition result.
- [ ] Regression test: a `while` loop that needs >10 iterations completes correctly.
- [ ] JS scenario test for a simple counter loop.

## Files

- `crates/quench-runtime/src/interpreter.rs`

## Verification

```bash
cargo test -p quench-runtime while_loop_counter
cargo test -p quench-runtime scenarios::while
```
