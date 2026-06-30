# Task 43: Validate conformance results with TypeScript's own runner

## Goal

Use the TypeScript repo's own `compilerRunner` to generate local baselines and confirm expected JS output for cases where Quench's harness results are ambiguous.

## Pareto & reuse note

- Prefer existing crates, the Rust standard library, and OS features over custom code.
- Follow the 80/20 rule: validate only the categories with failures or unclear baselines.
- Defer edge cases, but document them in this task or spawn a follow-up task so they are not lost.

## TDD & testing note

- Follow the red-green-refactor cycle: write a failing unit test first, then the minimal code to pass it, then refactor.
- Add a regression test for every bug fix and edge case covered by this task.
- Keep tests in `crates/quench-runtime/tests/` and run `cargo test -p quench-runtime` before marking work done.

## Files

- `tests/typescript/package.json`
- `tests/typescript/Herebyfile.mjs`
- `tests/typescript/src/testRunner/compilerRunner.ts`

## Commands to document and verify

```bash
cd tests/typescript
npm ci

# Build the test harness
npm run build:tests

# Run only the conformance runner
npx hereby runtests --runners=conformance

# Run a specific test by filename pattern
npx hereby runtests --runners=conformance --tests=abstractPropertyBasics

# Keep baselines between runs
npx hereby runtests --runners=conformance --tests=abstractPropertyBasics --dirty

# Inspect differences and accept new baselines
npx hereby diff
npx hereby baseline-accept
```

## Steps

1. Run the conformance runner for the categories Quench tests.
2. Inspect generated baselines in `tests/typescript/tests/baselines/local/`.
3. Compare with checked-in `tests/baselines/reference/` to confirm the expected JS format.
4. Document the commands and workflow in `docs/conformance.md` (Task 44).

## Boundaries

- Only use `tests/typescript/` for reference generation/verification.
- Do not commit changes inside `tests/typescript/`.
- Do not require the TypeScript runner as part of normal Quench CI.

## Acceptance criteria

- The commands above run successfully for at least one conformance test.
- A note exists explaining how to regenerate baselines when the submodule is updated.

## Timeout note

- All test commands must run with a timeout to avoid hangs from interpreter bugs or infinite loops.
- Use the `scripts/run_tests.sh` wrapper (if available) or prefix commands with `timeout 120` / `gtimeout 120`.
- In CI, set per-test and job-level timeouts (e.g., 5 minutes per test suite, 30 minutes per job).


## Verification

```bash
cd tests/typescript
npx hereby runtests --runners=conformance --tests=abstractPropertyBasics --dirty
```
