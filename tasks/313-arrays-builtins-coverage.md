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

## Targets

- **Suite:** `both`
- **Batch:** 6
- **Target subset:** `tests/test262/test/built-ins/Array; tests/test262/test/built-ins/String; tests/test262/test/built-ins/Number; tests/test262/test/built-ins/Boolean; tests/test262/test/built-ins/Date; tests/test262/test/built-ins/Error`. See `docs/conformance-coverage-matrix.md` for the exact file count.
- **Blocked by:** see `tasks/index.json`
- **Exit criteria:** All core built-in suites (Array, String, Number, Boolean, Date, Error) are active and pass at 100% with zero spec skips.
