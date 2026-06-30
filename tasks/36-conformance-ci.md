# Task 36: Gate conformance harness with pass-rate thresholds locally

## Goal

Make the conformance harness enforce a minimum pass rate locally, without relying on GitHub Actions or any external CI.

## Pareto & reuse note

- Prefer existing crates, the Rust standard library, and OS features over custom code.
- Follow the 80/20 rule: gate on the whitelist categories that are already mostly green; do not gate on classes until Task 18 is done.
- Defer edge cases, but document them in this task or spawn a follow-up task so they are not lost.

## TDD & testing note

- Follow the red-green-refactor cycle: write a failing unit test first, then the minimal code to pass it, then refactor.
- Add a regression test for every bug fix and edge case covered by this task.
- Keep tests in `crates/quench-runtime/tests/` and run `cargo test -p quench-runtime` before marking work done.

## Files

- `crates/quench-runtime/tests/conformance.rs`
- `scripts/run_tests.sh`
- `Makefile`

## Steps

1. Add a `MIN_PASS_RATE` environment variable (default e.g., 0.50) read by the conformance test.
2. After the harness finishes, compare the actual pass rate to the threshold.
3. If the pass rate is below the threshold, fail the test with a clear message showing current vs expected rate.
4. Wire the gate into `scripts/run_tests.sh` and/or `Makefile` so local runs can enforce it.
5. Document the gate and current baseline in `docs/conformance.md` (Task 44).

## Boundaries

- Only modify test harness code and local build scripts.
- Do not add GitHub Actions, Azure Pipelines, or any other external CI configuration.
- Do not modify `tests/typescript/`.

## Acceptance criteria

- Running the conformance harness with `MIN_PASS_RATE=0.50` fails when the pass rate is below 50%.
- Running with a threshold at or below the current rate passes.
- The gate is documented in `docs/conformance.md`.

## Verification

```bash
MIN_PASS_RATE=0.50 cargo test -p quench-runtime --test conformance -- --nocapture
```

All commands must run with a timeout (see Task 31).
