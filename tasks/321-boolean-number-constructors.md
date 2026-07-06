> New low-effort/high-impact win from code review.

# Task 321: Make Boolean and Number real constructors

## Status: PENDING

## Problem

`Boolean` is registered only as a `NativeFunction`, so there is no `Boolean.prototype` and `new Boolean()` fails. `Number` is registered as an object, but its `NativeConstructor` is created and discarded, so `new Number(5)` is broken.

## Fix

- Convert `Boolean` to a `NativeConstructor` with `Boolean.prototype` (`toString`, `valueOf`).
- Wire the existing `Number` constructor object into `ctx.set_global("Number", ...)` instead of discarding it.

## Acceptance criteria

- [ ] `new Boolean(true)` is a Boolean object.
- [ ] `new Number(5)` is a Number object.
- [ ] `Boolean.prototype.toString` and `Number.prototype.toString` exist.
- [ ] Regression tests and fixtures added.

## Files

- `crates/quench-runtime/src/builtins/date.rs` (`register_global_functions`)
- `crates/quench-runtime/src/builtins/number.rs`

## Effort / impact

- Effort: low
- Impact: high
