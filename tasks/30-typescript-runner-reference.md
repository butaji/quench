# Task 30: Run TypeScript's own conformance runner for reference

## Goal

Use the TypeScript repo's own test infrastructure to generate or verify baselines for selected conformance cases. This gives us a ground-truth reference when the checked-in baselines are missing, confusing, or out of sync with the shallow submodule.

## Pareto & reuse note

- Prefer existing crates, the Rust standard library, and OS features over custom code.
- Follow the 80/20 rule: only run the reference runner for categories where Quench's harness results are unclear.
- Defer edge cases, but document them in this task or spawn a follow-up task so they are not lost.

## TDD & testing note

- Follow the red-green-refactor cycle: write a failing unit test first, then the minimal code to pass it, then refactor.
- Add a regression test for every bug fix and edge case covered by this task.
- Keep tests in `crates/quench-runtime/tests/` and run `cargo test -p quench-runtime` before marking work done.

## Files

- `tests/typescript/package.json`
- `tests/typescript/src/testRunner/compilerRunner.ts`
- `tests/typescript/src/harness/harnessIO.ts`
- `tests/typescript/tests/baselines/reference/`

## Steps

1. Ensure the submodule is initialized and dependencies are installed:
   ```bash
   git submodule update --init tests/typescript
   cd tests/typescript
   npm ci
   ```
2. Run the TypeScript conformance runner for a single category or a single case:
   ```bash
   npx hereby runtests --tests=conformance
   # or
   npx hereby runtests --tests=conformance --test=expressions
   ```
   If `hereby` is not available, use:
   ```bash
   npx tsc -p src/testRunner/tsconfig.json
   node built/local/run.js --tests=conformance --test=expressions
   ```
3. For cases where the checked-in baseline is missing or unclear, use TypeScript's compiler to emit the JS:
   ```bash
   npx tsc tests/cases/conformance/es6/arrowFunction/emitArrowFunctionES6.ts \
     --target es2015 --module commonjs --outDir /tmp/ts-ref
   ```
4. Compare the reference output with the `.js` baseline in `tests/baselines/reference/` to confirm the baseline format.
5. Document the exact commands and any baseline differences in Task 30 notes.

## Boundaries

- Only use `tests/typescript/` for reference generation/verification.
- Do not commit changes inside `tests/typescript/`.
- Do not require the TypeScript runner as part of normal Quench CI; it is a development/reference tool only.

## Acceptance criteria

- The commands above run successfully for at least one conformance category.
- Reference baselines are generated for cases where Quench's harness results were ambiguous.
- The process is documented so any team member can reproduce it.

## Verification

```bash
cd tests/typescript
npm ci
npx hereby runtests --tests=conformance --test=expressions
```
