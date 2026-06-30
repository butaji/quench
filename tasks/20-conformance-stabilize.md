# Task 20: Stabilize conformance results and document supported subset

## Goal

Lock in the conformance gains, prevent regressions, and publish what is and is not supported.

## Pareto & reuse note

- Prefer existing crates, the Rust standard library, and OS features over custom code.
- Follow the 80/20 rule: implement the subset that unblocks the targeted examples/conformance tests first.
- Defer edge cases, but document them in this task or spawn a follow-up task so they are not lost.

## TDD & testing note

- Follow the red-green-refactor cycle: write a failing unit test first, then the minimal code to pass it, then refactor.
- Add a regression test for every bug fix and edge case covered by this task.
- Keep tests in `crates/quench-runtime/tests/` and run `cargo test -p quench-runtime` before marking work done.


## Files

- `.github/workflows/` (if CI exists)
- `crates/quench-runtime/tests/conformance.rs`
- `README.md`
- `EXECUTE.md`

## Steps

1. Fix any regressions caused by Tasks 17–19.
2. Add a CI job (or local script) that initializes the submodule and runs `cargo test -p quench-runtime --test conformance`.
3. Generate a conformance report: total cases, passed, failed, skipped, with per-category percentages.
4. Update `README.md` and `EXECUTE.md` with:
   - how to run conformance tests
   - the supported JS/TS subset
   - intentionally unsupported features (e.g., `with`, legacy octal, some strict-mode-only behaviors)
5. Mark `tests/typescript/` as read-only in contributor docs.

## Boundaries

- Only modify docs, CI, and test harness code.
- Do not modify `tests/typescript/` or `examples/`.

## Acceptance criteria

- `cargo test -p quench-runtime --test conformance` passes at the documented level.
- CI runs the conformance suite on every push.
- The supported subset is documented and matches reality.

## Verification

```bash
git submodule update --init tests/typescript
cargo test -p quench-runtime --test conformance
cat docs/conformance-report.md
```
