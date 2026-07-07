> Sub-task of 322: add `Function.prototype` default methods.

# Task 322b: Add Function.prototype default methods

## Status: PENDING

## Gap

`Function.prototype` is missing `call`, `apply`, `bind`, and `toString`. Many callbacks and built-in tests fail because of this.

## Fix

Install minimal native implementations on `Function.prototype` during registration.

## Acceptance criteria

- [ ] `(function(){}).call(null, 1, 2)` invokes the function with the given `this` and arguments.
- [ ] `(function(){}).apply(null, [1, 2])` invokes the function with the given `this` and array arguments.
- [ ] `(function(){}).bind({})` returns a bound function.
- [ ] `Function.prototype.toString` returns a placeholder string without crashing.
- [ ] Regression tests and fixtures added.

## Files

- `crates/quench-runtime/src/builtins/function.rs`

## Tests unblocked

- test262 `built-ins/Function/prototype/`
- TS baselines using `fn.call` / `fn.apply` / `fn.bind`
