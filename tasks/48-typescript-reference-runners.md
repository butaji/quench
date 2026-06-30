# Task 48: Run TypeScript compiler and project runners for reference baselines

## Goal

Use TypeScript's own `compilerRunner` and `projectRunner` to generate local baselines for `compiler/`, `conformance/`, `project/`, and `transpile/` cases. This confirms the expected JS output before Quench runs it.

## Pareto & reuse note

- Prefer existing crates, the Rust standard library, and OS features over custom code.
- Follow the 80/20 rule: run only the runners relevant to Quench's harness.
- Defer edge cases, but document them in this task or spawn a follow-up task so they are not lost.

## TDD & testing note

- Follow the red-green-refactor cycle: write a failing unit test first, then the minimal code to pass it, then refactor.
- Add a regression test for every bug fix and edge case covered by this task.
- Keep tests in `crates/quench-runtime/tests/` and run `cargo test -p quench-runtime` before marking work done.

## Files

- `tests/typescript/package.json`
- `tests/typescript/Herebyfile.mjs`
- `tests/typescript/src/testRunner/compilerRunner.ts`
- `tests/typescript/src/testRunner/projectsRunner.ts`

## Commands

```bash
cd tests/typescript
npm ci
npm run build:tests

# Conformance + compiler + project + transpile runners
npx hereby runtests --no-lint --runner=compiler,conformance,project,transpile

# Keep generated baselines between runs
npx hereby runtests --no-lint --runner=compiler --tests=2dArrays --dirty

# Inspect differences
npx hereby diff

# Accept new baselines (only if intentionally changing behavior)
npx hereby baseline-accept
```

## Steps

1. Run the combined reference runner for the categories Quench tests.
2. Inspect `tests/typescript/tests/baselines/local/`.
3. Compare with `tests/baselines/reference/` to confirm expected JS.
4. Document the commands in Task 50.

## Boundaries

- Only use `tests/typescript/` for reference generation.
- Do not commit changes inside `tests/typescript/`.

## Acceptance criteria

- The reference runners run successfully for `compiler`, `conformance`, `project`, and `transpile`.
- A note explains how to interpret `local/` vs `reference/` and how to accept baselines.

## Verification

```bash
cd tests/typescript
npx hereby runtests --no-lint --runner=compiler --tests=2dArrays --dirty
```
