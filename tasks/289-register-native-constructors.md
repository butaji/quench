# Task 289: Register Array, Error, Date as constructors

## Status: PENDING

## Gap

`new Array(3)`, `new Error("x")`, `new Date()` currently fail with `Object is not a constructor`. Built-ins are registered as plain `Value::Object`s.

## Fix

Register `Array`, `Error`, `Date` (and other built-ins) as callable constructors with proper `[[Construct]]` behavior, `prototype` object, and `constructor` link.

## Acceptance criteria

- [ ] `new Array(3)` creates `[empty × 3]`.
- [ ] `new Error("msg")` is an error instance.
- [ ] `new Date()` produces a date object.
- [ ] `instanceof` works against built-in prototypes.
- [ ] Regression tests for each constructor.
- [ ] JS scenario tests for `new Array` and `new Error`.

## Files

- `crates/quench-runtime/src/builtins/array.rs`
- `crates/quench-runtime/src/builtins/error.rs`
- `crates/quench-runtime/src/builtins/date.rs`
- `crates/quench-runtime/src/value.rs`
- `crates/quench-runtime/src/builtins/mod.rs`

## Tests unblocked

- test262 `built-ins/Array/*` (~3,000 tests)
- Error/Date built-in suites
- TS baselines using `new Array`
