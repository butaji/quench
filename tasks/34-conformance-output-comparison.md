# Task 34: Implement runtime output and error comparison for conformance cases

## Goal

For conformance cases that produce observable runtime behavior (console output or thrown errors), capture that behavior and compare it against the expected baseline.

## Pareto & reuse note

- Prefer existing crates, the Rust standard library, and OS features over custom code.
- Follow the 80/20 rule: many conformance cases have no runtime output; focus on cases that do.
- Defer edge cases, but document them in this task or spawn a follow-up task so they are not lost.

## TDD & testing note

- Follow the red-green-refactor cycle: write a failing unit test first, then the minimal code to pass it, then refactor.
- Add a regression test for every bug fix and edge case covered by this task.
- Keep tests in `crates/quench-runtime/tests/` and run `cargo test -p quench-runtime` before marking work done.

## Files

- `crates/quench-runtime/tests/conformance.rs`
- `crates/quench-runtime/src/context/mod.rs`
- `crates/quench-runtime/src/builtins/console.rs`

## Steps

1. Add a `ConsoleCapture` that intercepts `console.log`/`console.error` calls during test execution and stores them as strings.
2. Extend `TestResult` to include:
   - `OutputMismatch { expected: String, actual: String }`
   - `ErrorMismatch { expected: String, actual: String }`
3. For cases that have an expected-output baseline (rare in TypeScript conformance), compare captured output.
4. For cases that are expected to throw (error baselines), compare the thrown error message.
5. Keep the default behavior lenient: cases with no output/error baseline pass if they execute without crashing.

## Boundaries

- Only modify test harness and console capture code.
- Do not modify `tests/typescript/`.

## Acceptance criteria

- A case that calls `console.log` produces a captured string.
- A case expected to throw is marked failed only if the actual error differs from the expected error.

## Verification

```bash
cargo test -p quench-runtime --test conformance
```
