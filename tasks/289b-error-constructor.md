> Sub-task of 289: register `Error` as a constructor.

# Task 289b: Implement the Error constructor

## Status: PENDING

## Gap

`new Error("msg")` fails with `Object is not a constructor`. Error types are not registered as constructors, so runtime errors cannot be created by user code.

## Fix

- Register `Error` as a `NativeConstructor`.
- Implement `Error(message)` constructor semantics: create an object with `message` and `name` properties.
- Set up `Error.prototype` with `toString`.
- Optionally wire native error subclasses (`TypeError`, `ReferenceError`, `RangeError`, etc.) on top of the same mechanism.

## Acceptance criteria

- [ ] `new Error("msg").message === "msg"`.
- [ ] `new Error("msg").name === "Error"`.
- [ ] `new TypeError("x")` creates a `TypeError` instance.
- [ ] `err instanceof Error` works for error instances.
- [ ] Regression tests and JS scenario tests.

## Files

- `crates/quench-runtime/src/builtins/error.rs`
- `crates/quench-runtime/src/builtins/mod.rs`
- `crates/quench-runtime/src/value.rs`

## Tests unblocked

- test262 `built-ins/Error/`
- Runtime error creation in user code
