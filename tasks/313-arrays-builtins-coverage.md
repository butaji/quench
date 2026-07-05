> Sub-task of 296: reach 100% coverage of ECMA-262 arrays and built-in objects.

# Task 313: Arrays and built-ins coverage

## Status: PENDING

## Goal

100% of test262 `built-ins/Array/`, `built-ins/String/`, `built-ins/Number/`, `built-ins/Boolean/`, `built-ins/Date/`, `built-ins/Error/` tests pass.

## Scope

- Array constructors and prototype methods
- String prototype methods (UTF-16 aware)
- Number/Boolean/Date/Error constructors and prototypes
- `Array.flat`, `Array.flatMap`

## Acceptance criteria

- [ ] Built-ins core suites pass.
- [ ] Fixtures under `tests/spec_fixtures/arrays/` and `tests/spec_fixtures/errors/`.

## Dependencies

- Tasks 289, 283, 284, 147, 191, 132, 239
