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

## Targets

- **Suite:** `both`
- **Batch:** 6
- **Target subset:** `tests/test262/test/built-ins/Object; tests/test262/test/built-ins/Reflect`. See `docs/conformance-coverage-matrix.md` for the exact file count.
- **Blocked by:** see `tasks/index.json`
- **Exit criteria:** All test262 `built-ins/Object/` and `built-ins/Reflect/` files are active and pass at 100% with zero spec skips.
