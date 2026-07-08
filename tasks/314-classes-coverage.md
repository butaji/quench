> Sub-task of 296: reach 100% coverage of ECMA-262 classes.

# Task 314: Classes coverage

## Status: PENDING

## Goal

100% of test262 `language/class/` and `language/statements/class/` tests pass.

## Scope

- Class declarations and expressions
- Constructors, `super()`, `super.method()`
- `extends`, prototype chain setup
- Static fields and methods
- Public/private fields (future)

## Acceptance criteria

- [ ] Class-related tests pass.
- [ ] Fixtures under `tests/spec_fixtures/classes/`.

## Dependencies

- Tasks 182, 183, 187

## Targets

- **Suite:** `both`
- **Batch:** 6
- **Target subset:** `tests/test262/test/language/statements/class; tests/test262/test/language/expressions/class; tests/test262/test/language/computed-property-names/class`. See `docs/conformance-coverage-matrix.md` for the exact file count.
- **Blocked by:** see `tasks/index.json`
- **Exit criteria:** All test262 class syntax areas are active and pass at 100% with zero spec skips.
