> Sub-task of 289: register `Date` as a constructor.

# Task 289c: Implement the Date constructor

## Status: PENDING

## Gap

`new Date()` fails with `Object is not a constructor`. The `Date` built-in is not registered as a callable constructor.

## Fix

- Register `Date` as a `NativeConstructor`.
- Implement basic constructor semantics:
  - `new Date()` creates a date for the current time.
  - `new Date(ms)` creates a date from a timestamp.
  - `Date()` called as a function returns an implementation-defined date string.
- Attach `Date.prototype` and `Date.now`.

## Acceptance criteria

- [ ] `new Date()` returns a Date object.
- [ ] `new Date(0)` returns the epoch Date object.
- [ ] `Date.now()` returns a number.
- [ ] `date instanceof Date` works.
- [ ] Regression tests and JS scenario tests.

## Files

- `crates/quench-runtime/src/builtins/date.rs`
- `crates/quench-runtime/src/builtins/mod.rs`
- `crates/quench-runtime/src/value.rs`

## Tests unblocked

- test262 `built-ins/Date/` constructor tests
- TS baselines using `new Date`
