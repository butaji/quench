> Sub-task of 296: reach 100% coverage of ECMA-262 objects and prototypes.

# Task 312: Objects coverage

## Status: PENDING

## Goal

100% of test262 `built-ins/Object/`, `built-ins/Reflect/`, and prototype-chain tests pass.

## Scope

- Object literals, property access, computed keys
- Prototype chain, `__proto__`, `Object.create`
- `Object.defineProperty`, `getOwnPropertyDescriptor`
- `Object.keys`, `Object.prototype` methods
- `Reflect` helpers

## Acceptance criteria

- [ ] Object semantics tests pass.
- [ ] Fixtures under `tests/spec_fixtures/objects/`.

## Dependencies

- Tasks 294, 295, 289
