> Sub-task of 296: reach 100% coverage of JSX/TSX runtime behavior.

# Task 319: JSX coverage

## Status: PENDING

## Goal

100% of JSX/TSX examples and tests render correctly via the Ink runtime.

## Scope

- JSX elements transformed to `ink.createElement` calls
- Component composition
- Props/spread props
- Children handling

## Acceptance criteria

- [ ] All JSX examples run natively.
- [ ] Fixtures under `tests/spec_fixtures/jsx/`.

## Dependencies

- Task 318 (TypeScript transform foundation)
