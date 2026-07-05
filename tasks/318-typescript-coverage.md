> Sub-task of 296: reach 100% coverage of TypeScript-specific runtime behavior.

# Task 318: TypeScript coverage

## Status: PENDING

## Goal

100% of TypeScript runtime conformance cases pass after type erasure.

## Scope

- Type annotations erased
- Enums (numeric and string)
- Namespaces
- `as const`, `satisfies`, type assertions
- TSX/JSX transform
- `declare` and ambient modules dropped

## Acceptance criteria

- [ ] Full TypeScript conformance suite passes.
- [ ] Fixtures under `tests/spec_fixtures/typescript/`.

## Dependencies

- Tasks 290, 289, 241, 182
