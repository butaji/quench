# Task 32: Expand conformance harness to full whitelist run

## Goal

Remove the 200-case limit used in the Task 16 audit and run the conformance harness over every runtime-relevant case in the whitelist.

## Pareto & reuse note

- Prefer existing crates, the Rust standard library, and OS features over custom code.
- Follow the 80/20 rule: start with categories that are already mostly green; expect classes to dominate failures until Task 18 is done.
- Defer edge cases, but document them in this task or spawn a follow-up task so they are not lost.

## TDD & testing note

- Follow the red-green-refactor cycle: write a failing unit test first, then the minimal code to pass it, then refactor.
- Add a regression test for every bug fix and edge case covered by this task.
- Keep tests in `crates/quench-runtime/tests/` and run `cargo test -p quench-runtime` before marking work done.

## Files

- `crates/quench-runtime/tests/conformance.rs`
- `tasks/audit/conformance-audit.md`

## Steps

1. Remove or parameterize the `limit` used in `run_whitelist_conformance_tests`.
2. Add a new test entry point `test_full_whitelist_conformance` that calls the runner with no limit.
3. Ensure the runner prints progress every 100 cases and a final summary.
4. Run the full whitelist and record:
   - total discovered cases
   - passed / failed / skipped counts
   - failures grouped by category
5. Update `tasks/audit/conformance-audit.md` with the full-run results (or create a new `docs/conformance-full-run.md`).

## Boundaries

- Only modify test harness code.
- Do not modify `tests/typescript/`.

## Acceptance criteria

- `cargo test -p quench-runtime --test conformance -- test_full_whitelist_conformance --nocapture` runs to completion.
- A full-run report is written and committed in `tasks/` or `docs/`.

## Verification

```bash
./scripts/run_tests.sh test_full_whitelist_conformance
```
