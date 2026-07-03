# Task 279: Enforce granular unit-test policy

## Status: IN PROGRESS

## Goal

Make the project's testing culture granular enough that any regression can be tracked to a single, isolated test. Every doc and task that governs implementation must require focused unit tests.

## Policy

1. **One behavior, one test.** A unit test should fail for exactly one reason.
2. **Small and isolated.** Tests do not depend on global state, file system, or network. They use in-memory `Runtime` or direct function calls.
3. **Named after the invariant.** A test name describes the behavior it protects, e.g., `number_addition_coerces_string_operand`, `try_catch_preserves_thrown_object`.
4. **Single-test reproducibility.** Any regression must be reproducible with:
   ```bash
   cargo test -p quench-runtime <test_name>
   ```
5. **No fix without a failing test.** Every bug fix and feature starts with a test that fails before the change and passes after.
6. **Task acceptance criteria include tests.** Each task in `tasks/index.json` must list the test file(s) and test name(s) added or modified.

## Documentation updates

- `EXECUTE.md` — principle 8 now requires granular, test-driven development; added a Testing policy section.
- `docs/hir-design.md` — each HIR implementation phase lists required focused tests.

## Verification

- [ ] Every open task in `tasks/index.json` notes the tests it requires.
- [ ] New runtime bug fixes include a regression test in `crates/quench-runtime/tests/`.
- [ ] New HIR phases include unit tests for each op/data structure before the phase is marked complete.

## Dependencies

- Applies to all current and future tasks, especially Tasks 256, 264, 85, 88, 251, 241, 82.
