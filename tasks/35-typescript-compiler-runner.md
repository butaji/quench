# Task 35: Run TypeScript's own compiler runner for baseline validation

## Goal

Use the TypeScript repo's own `compilerRunner` to generate or verify `.js` baselines for selected conformance categories. This gives a ground-truth reference when Quench's harness results are ambiguous.

## Pareto & reuse note

- Prefer existing crates, the Rust standard library, and OS features over custom code.
- Follow the 80/20 rule: only run the reference runner for categories where the Quench harness is unclear.
- Defer edge cases, but document them in this task or spawn a follow-up task so they are not lost.

## TDD & testing note

- Follow the red-green-refactor cycle: write a failing unit test first, then the minimal code to pass it, then refactor.
- Add a regression test for every bug fix and edge case covered by this task.
- Keep tests in `crates/quench-runtime/tests/` and run `cargo test -p quench-runtime` before marking work done.

## Files

- `tests/typescript/package.json`
- `tests/typescript/Herebyfile.mjs`
- `tests/typescript/src/testRunner/compilerRunner.ts`
- `tests/typescript/src/harness/harnessIO.ts`

## Steps

1. Install TypeScript repo dependencies:
   ```bash
   cd tests/typescript
   npm ci
   ```
2. Build the test runner:
   ```bash
   npx hereby build:tests
   # or
   npx tsc -p src/testRunner/tsconfig.json
   ```
3. Run the compiler runner for the categories Quench cares about:
   ```bash
   npx hereby runtests --tests=conformance --test=expressions
   npx hereby runtests --tests=conformance --test=statements
   npx hereby runtests --tests=conformance --test=functions
   npx hereby runtests --tests=conformance --test=classes
   ```
4. Inspect the generated baselines under `tests/typescript/tests/baselines/local/`.
5. Compare them with the checked-in `tests/baselines/reference/` to confirm expected JS output.
6. Document the exact commands and any differences in Task 35 notes.

## Boundaries

- Only use `tests/typescript/` for reference generation/verification.
- Do not commit changes inside `tests/typescript/`.
- Do not require the TypeScript runner as part of normal Quench CI.

## Acceptance criteria

- The reference runner runs successfully for at least one conformance category.
- A note explains how to regenerate baselines when the TypeScript submodule is updated.

## Verification

```bash
cd tests/typescript
npm ci
npx hereby runtests --tests=conformance --test=classes
```
