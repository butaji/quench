> Sub-task of 289: register `Array` as a constructor.

# Task 289a: Implement the Array constructor

## Status: PENDING

## Gap

`new Array(3)` and `new Array(1, 2, 3)` fail with `Object is not a constructor`. `Array` is registered as a plain object instead of a callable constructor.

## Fix

- Register `Array` as a `NativeConstructor` with `[[Construct]]` and `[[Call]]` behaviors.
- Implement the constructor semantics:
  - `new Array(len)` creates an array with `length` set to `len` and no indexed elements.
  - `new Array(a, b, c)` creates an array `[a, b, c]`.
  - `Array(...)` called as a function behaves the same as `new Array(...)`.
- Link `Array.prototype` and ensure `array instanceof Array` works.

## Acceptance criteria

- [ ] `new Array(3)` creates `[empty × 3]` with `length === 3`.
- [ ] `new Array(1, 2, 3)` creates `[1, 2, 3]`.
- [ ] `Array(1, 2)` called without `new` returns `[1, 2]`.
- [ ] `[] instanceof Array` and `new Array() instanceof Array` are true.
- [ ] Regression tests and JS scenario tests.

## Files

- `crates/quench-runtime/src/builtins/array.rs`
- `crates/quench-runtime/src/builtins/mod.rs`
- `crates/quench-runtime/src/value.rs`

## Tests unblocked

- test262 `built-ins/Array/` constructor tests
- TS baselines using `new Array`
