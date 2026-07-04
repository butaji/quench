# Task 292: Implement var hoisting and let/const TDZ

## Status: PENDING

## Gap

`var` declarations are not hoisted. `let`/`const` are treated like `var` with no temporal dead zone (TDZ) checks.

## Fix

- Hoist `var` initializers as `undefined` during scope setup.
- Track `let`/`const` bindings and throw `ReferenceError` on TDZ access.

## Acceptance criteria

- [ ] `console.log(x); var x = 1;` logs `undefined`.
- [ ] `x; let x = 1;` throws `ReferenceError` (TDZ).
- [ ] `const x = 1; x = 2;` throws `TypeError`.
- [ ] Regression tests and JS scenario tests for hoisting and TDZ.

## Files

- `crates/quench-runtime/src/interpreter/statements.rs`
- `crates/quench-runtime/src/env.rs`
- `crates/quench-runtime/src/value.rs`

## Tests unblocked

- Large swath of variable-scope tests
- Many "x is not defined" TS failures
