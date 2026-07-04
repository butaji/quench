# Task 282: Share function bodies via Rc<[Statement]>

## Status: PENDING

## Problem

`ValueFunction.body: Vec<Statement>` is cloned by derive-style `Clone`. Every time a function value is passed, its entire statement vector is copied.

## Fix

Store function bodies as `Rc<[Statement]>` (and arrow bodies similarly). Cloning a function value then becomes a cheap `Rc` bump.

## Acceptance criteria

- [ ] `ValueFunction` and arrow function bodies use `Rc<[Statement]>`.
- [ ] `Clone` impl is removed or derived and cheap.
- [ ] Regression test measures that cloning a large function body is O(1).
- [ ] JS scenario test with closures returned from loops still works.

## Files

- `crates/quench-runtime/src/value.rs`
- `crates/quench-runtime/src/interpreter.rs`
- `crates/quench-runtime/src/lower.rs`

## Verification

```bash
cargo test -p quench-runtime function_body_shared
cargo test -p quench-runtime scenarios::closure
```
