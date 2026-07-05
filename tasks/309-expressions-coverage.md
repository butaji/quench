> Sub-task of 296: reach 100% coverage of ECMA-262 expressions.

# Task 309: Expressions coverage

## Status: PENDING

## Goal

100% of test262 `language/expressions/` and TypeScript expression tests pass.

## Scope

- Unary operators (`typeof`, `delete`, `void`, `+`, `-`, `!`, `~`)
- Binary operators (`+`, `-`, `*`, `/`, `%`, `**`, comparison, equality, bitwise, logical)
- Nullish coalescing (`??`) and optional chaining (`?.`)
- Template literals
- Spread in arrays/objects/calls
- Computed property keys
- `new`, `super`, `this`

## Acceptance criteria

- [ ] All `language/expressions/` tests in the current subset pass.
- [ ] Fixture exists for each expression form under `tests/spec_fixtures/expressions/`.
- [ ] TS expression failures reduced to zero.

## Dependencies

- Tasks 290, 291, 289, 283
