# Task 36: Integrate conformance harness into CI with thresholds

## Goal

Add a continuous-integration job that runs the TypeScript conformance harness and fails the build if the pass rate drops below a documented threshold.

## Pareto & reuse note

- Prefer existing crates, the Rust standard library, and OS features over custom code.
- Follow the 80/20 rule: gate on the whitelist categories that are already mostly green; do not gate on classes until Task 18 is done.
- Defer edge cases, but document them in this task or spawn a follow-up task so they are not lost.

## TDD & testing note

- Follow the red-green-refactor cycle: write a failing unit test first, then the minimal code to pass it, then refactor.
- Add a regression test for every bug fix and edge case covered by this task.
- Keep tests in `crates/quench-runtime/tests/` and run `cargo test -p quench-runtime` before marking work done.

## Files

- `.github/workflows/` (or existing CI files)
- `crates/quench-runtime/tests/conformance.rs`
- `scripts/run_tests.sh`

## Steps

1. Add a CI job (GitHub Actions) that:
   - Checks out the repo with submodules (`submodules: recursive`).
   - Installs Rust.
   - Runs `cargo test -p quench-runtime --test conformance -- --nocapture`.
2. Make the harness exit with a non-zero status if the pass rate is below the threshold. Store the threshold in the test or as an environment variable.
3. Start with a threshold that matches the current baseline (e.g., 56% for the 200-case audit, or the full-whitelist pass rate after Task 32).
4. Update `README.md`/`EXECUTE.md` with the CI badge and conformance status.

## Boundaries

- Only modify CI/workflow files and harness exit behavior.
- Do not modify `tests/typescript/`.

## Acceptance criteria

- A CI job runs the conformance harness on every push.
- The job fails if the pass rate drops below the threshold.

## Timeout note

- All test commands must run with a timeout to avoid hangs from interpreter bugs or infinite loops.
- Use the `scripts/run_tests.sh` wrapper (if available) or prefix commands with `timeout 120` / `gtimeout 120`.
- In CI, set per-test and job-level timeouts (e.g., 5 minutes per test suite, 30 minutes per job).


## Verification

```bash
# Simulate CI locally
cargo test -p quench-runtime --test conformance -- --nocapture
```
