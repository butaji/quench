> Sub-task of 296: reach 100% coverage of ECMA-262 statements.

# Task 310: Statements coverage

## Status: PENDING

## Goal

100% of test262 `language/statements/` and TypeScript statement tests pass.

## Scope

- `if`, `while`, `do-while`, `for`, `for-in`, `for-of`
- `break`/`continue` with labels
- `switch`
- `try/catch/finally`
- `with` (optional, low priority)
- Block scoping, `var` hoisting, TDZ

## Acceptance criteria

- [ ] All `language/statements/` tests in the current subset pass.
- [ ] Fixture exists for each statement form under `tests/spec_fixtures/statements/`.

## Dependencies

- Tasks 281, 292, 250, 141

## Targets

- **Suite:** `both`
- **Batch:** 6
- **Target subset:** `tests/test262/test/language/statements`. See `docs/conformance-coverage-matrix.md` for the exact file count.
- **Blocked by:** see `tasks/index.json`
- **Exit criteria:** All test262 `language/statements/` files are active and pass at 100% with zero spec skips.
