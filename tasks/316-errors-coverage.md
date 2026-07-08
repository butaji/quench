> Sub-task of 296: reach 100% coverage of ECMA-262 errors and exceptions.

# Task 316: Errors and exceptions coverage

## Status: PENDING

## Goal

100% of test262 `language/exceptions/` and `built-ins/Error/` tests pass.

## Scope

- `try/catch/finally`
- Thrown value preservation
- `Error`, `TypeError`, `ReferenceError`, `RangeError`, `SyntaxError` constructors
- Error stack traces

## Acceptance criteria

- [ ] Error/exception tests pass.
- [ ] Fixtures under `tests/spec_fixtures/errors/`.

## Dependencies

- Tasks 250, 132, 105, 112

## Targets

- **Suite:** `both`
- **Batch:** 6
- **Target subset:** `tests/test262/test/built-ins/Error; tests/test262/test/built-ins/NativeErrors`. See `docs/conformance-coverage-matrix.md` for the exact file count.
- **Blocked by:** see `tasks/index.json`
- **Exit criteria:** All test262 `built-ins/Error/` and `built-ins/NativeErrors/` files are active and pass at 100% with zero spec skips.
