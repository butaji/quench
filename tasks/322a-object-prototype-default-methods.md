> Sub-task of 322: add `Object.prototype` default methods.

# Task 322a: Add Object.prototype default methods

## Status: PENDING

## Gap

`Object.prototype` is missing default methods that built-in and user code expect: `toString`, `valueOf`, `hasOwnProperty`, `isPrototypeOf`, `propertyIsEnumerable`.

## Fix

Install minimal native implementations on `Object.prototype` during registration.

## Acceptance criteria

- [ ] `({}).toString()` returns `"[object Object]"`.
- [ ] `({}).valueOf()` returns the object itself.
- [ ] `({}).hasOwnProperty("x")` works for own properties.
- [ ] `Object.prototype.isPrototypeOf` and `propertyIsEnumerable` installed.
- [ ] Regression tests and fixtures added.

## Files

- `crates/quench-runtime/src/builtins/object.rs`

## Tests unblocked

- test262 `built-ins/Object/prototype/`
- TS baselines using `hasOwnProperty` / `toString`
