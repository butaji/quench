# Task 294: Implement property descriptors and defineProperty

## Status: PENDING

## Gap

There are no property descriptors. Getters/setters are lowered as plain functions. `Object.defineProperty` ignores `get`/`set`/`enumerable`/`configurable`. `Object.getOwnPropertyDescriptor` is missing.

## Fix

Add descriptor objects to the object model and use them in `get`/`set`/`has`/`delete`. Implement `Object.getOwnPropertyDescriptor` and real `Object.defineProperty`.

## Acceptance criteria

- [ ] `Object.defineProperty(o, 'x', { value: 1, writable: false })` enforces writability.
- [ ] Getter/setter descriptors work.
- [ ] `Object.getOwnPropertyDescriptor` returns the descriptor.
- [ ] Regression tests and JS scenario tests.

## Files

- `crates/quench-runtime/src/value.rs`
- `crates/quench-runtime/src/builtins/object.rs`
- `crates/quench-runtime/src/interpreter/member.rs`

## Tests unblocked

- Object-semantics tests
- Built-in tests that verify property attributes
