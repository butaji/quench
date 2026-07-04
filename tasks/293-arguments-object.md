# Task 293: Implement arguments object in functions

## Status: PENDING

## Gap

Functions have no `arguments` object. Accessing `arguments` inside a non-arrow function throws.

## Fix

Create an array-like `arguments` object in `call_js_function` and bind it in the function scope before executing the body.

## Acceptance criteria

- [ ] `function f() { return arguments.length; }` works.
- [ ] `arguments[i]` returns the i-th positional argument.
- [ ] Mutating `arguments[i]` does not affect named parameters (non-strict) or is forbidden (strict) per spec.
- [ ] Regression tests and JS scenario tests.

## Files

- `crates/quench-runtime/src/interpreter/func.rs`
- `crates/quench-runtime/src/value.rs`

## Tests unblocked

- Legacy function-argument tests
- TS `functionCalls` failures
