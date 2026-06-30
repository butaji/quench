# Task 45: Port TypeScript evaluation unit tests to Quench harness

## Goal

Use the TypeScript repo's existing evaluation unit tests (`src/testRunner/unittests/evaluation/*.ts`) as a starter runtime-coverage suite. These tests already compile TypeScript and execute the emitted JS, so they provide immediate, known-good runtime validation for Quench.

## Pareto & reuse note

- Prefer existing crates, the Rust standard library, and OS features over custom code.
- Follow the 80/20 rule: port the smallest evaluation test file first to prove the integration.
- Defer edge cases, but document them in this task or spawn a follow-up task so they are not lost.

## TDD & testing note

- Follow the red-green-refactor cycle: write a failing unit test first, then the minimal code to pass it, then refactor.
- Add a regression test for every bug fix and edge case covered by this task.
- Keep tests in `crates/quench-runtime/tests/` and run `cargo test -p quench-runtime` before marking work done.

## Files

- `tests/typescript/src/testRunner/unittests/evaluation/*.ts`
- `crates/quench-runtime/tests/evaluation.rs` (new)
- `crates/quench-runtime/tests/conformance.rs`

## Steps

1. List the evaluation test files under `tests/typescript/src/testRunner/unittests/evaluation/`.
2. Pick one simple file (e.g., `evalTests.ts` or `iterationTests.ts`) and extract its TypeScript snippets.
3. For each snippet:
   - Compile it with the in-repo TypeScript API (`ts.transpileModule` or `ts.createProgram`).
   - Run the emitted JS in a fresh `quench_runtime::Context`.
   - Assert no runtime error.
4. Add a Rust test entry point `test_evaluation_unit` that runs all ported snippets.
5. As the runtime improves, port more evaluation test files.

## Boundaries

- Only add test harness code.
- Do not modify `tests/typescript/`.

## Acceptance criteria

- At least one TypeScript evaluation unit test file is ported and passes end-to-end.
- A report shows how many evaluation snippets passed/failed.

## Verification

```bash
cargo test -p quench-runtime --test evaluation -- --nocapture
```
