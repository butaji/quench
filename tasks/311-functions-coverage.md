> Sub-task of 296: reach 100% coverage of ECMA-262 functions.

# Task 311: Functions coverage

## Status: PENDING

## Goal

100% of test262 `language/functions/` and TypeScript function tests pass.

## Scope

- Function declarations, expressions, arrows
- Default parameters, rest parameters, parameter destructuring
- `arguments` object
- `call`/`apply`/`bind`
- Closures and upvalues
- Generator functions (future)

## Acceptance criteria

- [ ] All `language/functions/` tests in the current subset pass.
- [ ] Fixtures under `tests/spec_fixtures/functions/` for each feature.

## Dependencies

- Tasks 293, 292, 305 (rest), 119/117, 124/187
