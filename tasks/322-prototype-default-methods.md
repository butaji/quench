> New low-effort/high-impact win from code review.

# Task 322: Add Object and Function prototype default methods

## Status: PENDING

## Problem

`Object.prototype` lacks `toString`, `valueOf`, `hasOwnProperty`, `isPrototypeOf`, `propertyIsEnumerable`. `Function.prototype` lacks `call`, `apply`, `bind`, `toString`. Many built-in and user tests fail because these defaults are missing.

## Fix

Install minimal native implementations in `Object.prototype` and `Function.prototype` during registration.

## Acceptance criteria

- [ ] `({}).toString()` returns `"[object Object]"`.
- [ ] `(function(){}).call(null, 1)` works.
- [ ] `(function(){}).bind({})` returns a bound function.
- [ ] Regression tests and fixtures added.

## Files

- `crates/quench-runtime/src/builtins/object.rs`
- `crates/quench-runtime/src/builtins/function.rs`

## Effort / impact

- Effort: low–medium
- Impact: high
