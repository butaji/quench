# Task 15: Add TypeScript conformance submodule and test harness

## Goal

Bring the official TypeScript test corpus into the repo and build a Rust runner that can parse and interpret conformance cases natively in `quench-runtime`, using TypeScript's own baselines as the source of truth.

## Pareto & reuse note

- Prefer existing crates, the Rust standard library, and OS features over custom code.
- Follow the 80/20 rule: implement the subset that unblocks the targeted examples/conformance tests first.
- Defer edge cases, but document them in this task or spawn a follow-up task so they are not lost.

## TDD & testing note

- Follow the red-green-refactor cycle: write a failing unit test first, then the minimal code to pass it, then refactor.
- Add a regression test for every bug fix and edge case covered by this task.
- Keep tests in `crates/quench-runtime/tests/` and run `cargo test -p quench-runtime` before marking work done.

## Files

- `.gitmodules`
- `tests/typescript/` (shallow submodule)
- `crates/quench-runtime/tests/conformance.rs` (new test harness)
- `crates/quench-runtime/Cargo.toml` (dev-dependencies: `walkdir`, `regex` or `lazy_static`)

## What the TypeScript submodule provides

- `tests/typescript/tests/cases/conformance/**/*.ts{x}` — input test cases.
- `tests/typescript/tests/baselines/reference/*.js` — expected emitted JS for each case.
- `tests/typescript/tests/baselines/reference/*.errors.txt` — expected type errors (used only to skip negative tests).
- `tests/typescript/src/testRunner/compilerRunner.ts` — TypeScript's own runner (reference only).
- `tests/typescript/src/harness/harnessIO.ts` — directive parser rules.

## Steps

1. Add the TypeScript repo as a shallow submodule (already done if present):
   ```bash
   git submodule add --depth 1 https://github.com/microsoft/TypeScript.git tests/typescript
   ```
2. Commit `.gitmodules` and the submodule pointer.
3. Create `crates/quench-runtime/tests/conformance.rs` with a harness that:
   - Walks `tests/typescript/tests/cases/conformance/**/*.ts` and `**/*.tsx`.
   - Parses `// @name: value` directives from each file (case-insensitive, comma-separated values for multi-config cases).
   - Splits multi-file cases on `// @filename:` markers into virtual source files.
   - Looks up the corresponding `.js` baseline in `tests/typescript/tests/baselines/reference/`.
   - Skips cases that are not runtime-relevant:
     - non-empty `.errors.txt` baseline for the chosen configuration
     - `// @noEmit: true`
     - `// @emitDeclarationOnly: true`
     - unsupported module systems (`amd`, `umd`, `system`, `node16`, `nodenext`)
     - JSX without a runtime stub
     - decorators/metadata without helper stubs
     - type-check-only directories (`types`, `interfaces`, `Symbols`, `declarationEmit`, `additionalChecks`, `pedantic`, `jsdoc`, `salsa`, `typings`, `override`)
   - Extracts the emitted JS sections from the baseline (split on `//// [filename.js]` headers, normalize CRLF → LF).
   - Feeds the extracted JS directly into a fresh `quench_runtime::Context` and runs it to completion.
   - Reports pass / fail / skip with file paths and captured errors.
4. Add a single sanity test that runs one trivial case, e.g.:
   - `tests/cases/conformance/es6/arrowFunction/emitArrowFunctionES6.ts`
5. Document how to initialize the submodule in `EXECUTE.md` and `README.md`.

## Boundaries

- Do not modify files inside `tests/typescript/`.
- Do not change runtime behavior in this task; the harness may report failures.
- `examples/` remain immutable.

## Acceptance criteria

- `git submodule update --init tests/typescript` succeeds and the directory is populated.
- `cargo test -p quench-runtime --test conformance` discovers files, filters them, and runs at least one sanity case.
- The harness prints a summary of passed/failed/skipped cases.

## Verification

```bash
git submodule update --init tests/typescript
cargo test -p quench-runtime --test conformance
```
