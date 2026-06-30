# Task 40: Provide TypeScript emit helpers in conformance context

## Goal

Preload the helper functions that TypeScript injects into emitted JS (e.g., `__extends`, `__awaiter`) so baseline JS runs without `ReferenceError`.

## Pareto & reuse note

- Prefer existing crates, the Rust standard library, and OS features over custom code.
- Follow the 80/20 rule: start with the most common helpers (`__extends`, `__assign`, `__awaiter`, `__generator`); add others as failures show they are needed.
- Defer edge cases, but document them in this task or spawn a follow-up task so they are not lost.

## TDD & testing note

- Follow the red-green-refactor cycle: write a failing unit test first, then the minimal code to pass it, then refactor.
- Add a regression test for every bug fix and edge case covered by this task.
- Keep tests in `crates/quench-runtime/tests/` and run `cargo test -p quench-runtime` before marking work done.

## Background

When TypeScript targets older ES versions it emits helper calls:

- `__extends` for class inheritance
- `__assign`, `__spreadArrays` for object/array spread
- `__awaiter`, `__generator` for async/await and generators
- `__decorate`, `__metadata`, `__param` for decorators
- `__importStar`, `__importDefault`, `__exportStar` for modules

Definitions live in `tests/typescript/src/compiler/factory/emitHelpers.ts`. If a test uses `// @importHelpers: true`, helpers are imported from `tslib` instead.

## Files

- `crates/quench-runtime/tests/conformance.rs`
- `crates/quench-runtime/src/context/mod.rs` (if a helper registration helper is added)

## Steps

1. Create a `helpers.js` string or Rust constants with pure-JS implementations of the most common helpers.
2. Before evaluating any baseline JS, run the helpers in the test `Context`.
3. Skip cases with `@importHelpers: true` until `tslib` is available.
4. Add a unit test that runs a minimal baseline using `__extends` and passes.

## Boundaries

- Only modify test harness code; optionally add a helper-injection API to `Context`.
- Do not modify `tests/typescript/`.

## Acceptance criteria

- A conformance case that emits `__extends` runs without `ReferenceError`.
- Cases with `@importHelpers: true` are skipped with a clear reason.

## Verification

```bash
cargo test -p quench-runtime --test conformance
```
