# Task 41: Tighten conformance skip rules for non-runnable cases

## Goal

Avoid running TypeScript conformance cases that cannot produce meaningful runtime results, and report exactly why each is skipped.

## Pareto & reuse note

- Prefer existing crates, the Rust standard library, and OS features over custom code.
- Follow the 80/20 rule: implement the skip rules that remove the largest number of false failures first.
- Defer edge cases, but document them in this task or spawn a follow-up task so they are not lost.

## TDD & testing note

- Follow the red-green-refactor cycle: write a failing unit test first, then the minimal code to pass it, then refactor.
- Add a regression test for every bug fix and edge case covered by this task.
- Keep tests in `crates/quench-runtime/tests/` and run `cargo test -p quench-runtime` before marking work done.

## Files

- `crates/quench-runtime/tests/conformance.rs`

## Skip rules to add

1. **No JS baseline** — if the only baseline is `.errors.txt`, skip as diagnostic-only.
2. **`@noEmit: true` / `@emitDeclarationOnly: true`** — already skipped; keep and add to summary.
3. **`@importHelpers: true`** — skip until tslib is available.
4. **Unsupported module systems** — `amd`, `umd`, `system`, `node16`, `nodenext`; `commonjs` may also need a stub `require`.
5. **JSX with React runtime** — skip unless a React/JSX stub is registered.
6. **Decorators/metadata** — skip unless decorator helpers are available.
7. **`@target` below runtime support** — if the runtime cannot handle the emitted helpers, skip or preload helpers.
8. **Reference paths to external files** — skip if the referenced file is not in the baseline.

## Steps

1. Refactor `should_skip` to return a structured `SkipReason` enum instead of a string.
2. Add each rule above with a clear message.
3. Aggregate skip reasons in the final report.
4. Add unit tests for skip-rule detection.

## Boundaries

- Only modify test harness code.
- Do not modify `tests/typescript/`.

## Acceptance criteria

- Diagnostic-only cases are skipped with reason `diagnostic-only`.
- `@importHelpers: true` cases are skipped with reason `importHelpers`.
- Skip reasons are grouped in the final report.

## Timeout note

- All test commands must run with a timeout to avoid hangs from interpreter bugs or infinite loops.
- Use the `scripts/run_tests.sh` wrapper (if available) or prefix commands with `timeout 120` / `gtimeout 120`.
- In CI, set per-test and job-level timeouts (e.g., 5 minutes per test suite, 30 minutes per job).


## Verification

```bash
cargo test -p quench-runtime --test conformance -- --nocapture
```
