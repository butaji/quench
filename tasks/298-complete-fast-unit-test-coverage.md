> Ensure every JS/TS/TSX/JSX spec behavior is covered by a fast Rust unit test.

# Task 298: Complete fast Rust unit test coverage

## Status: IN PROGRESS

## Goal

The project must have complete coverage of fast Rust unit tests so that every spec-compatibility behavior is verified and regressions are caught immediately.

## Scope

- **Inline `#[test]` modules** for internal data structures (Value, Environment, shapes, AST lowering).
- **Integration tests** in `crates/quench-runtime/tests/` for interpreter-level behavior.
- **Spec fixtures** in `crates/quench-runtime/tests/spec_fixtures/` for ECMA-262 and TypeScript language snippets.
- **Scenario tests** in `crates/quench-runtime/tests/scenarios/` for end-to-end user-facing snippets.

## Rules

1. **No feature or fix without a test.** If the runtime changes, a test must fail before the fix and pass after.
2. **Fast feedback.** Every test runs in under a few seconds. Prefer `cargo test -p quench-runtime <test_name>`.
3. **One behavior, one test.** A test fails for exactly one reason.
4. **Spec-mapped.** Every fixture maps to an ECMA-262 or TypeScript spec section.

## Acceptance criteria

- [ ] Every P0/P1 compatibility task in `tasks/index.json` lists the test(s)/fixture(s) it requires.
- [ ] Spec fixtures exist for all top compatibility gaps (typeof, while, constructors, expressions, etc.).
- [ ] Running `cargo test -p quench-runtime` completes quickly and reports coverage.

## Dependencies

- Task 279 (granular unit-test policy)
- Task 297 (spec test fixtures)
- All P0/P1 compatibility tasks
