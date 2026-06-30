# Task 47: Add Quench harness for TypeScript project/ multi-file cases

## Goal

Run the JSON-driven project specs in `tests/typescript/tests/cases/project/` and `tests/cases/projects/` by compiling multi-file projects and executing the emitted JS in Quench with a minimal CommonJS loader.

## Pareto & reuse note

- Prefer existing crates, the Rust standard library, and OS features over custom code.
- Follow the 80/20 rule: start with `project/` specs that use CommonJS; skip AMD/System-only specs initially.
- Defer edge cases, but document them in this task or spawn a follow-up task so they are not lost.

## TDD & testing note

- Follow the red-green-refactor cycle: write a failing unit test first, then the minimal code to pass it, then refactor.
- Add a regression test for every bug fix and edge case covered by this task.
- Keep tests in `crates/quench-runtime/tests/` and run `cargo test -p quench-runtime` before marking work done.

## Files

- `tests/typescript/tests/cases/project/**/*.json`
- `tests/typescript/tests/cases/projects/`
- `crates/quench-runtime/tests/project.rs` (new)

## Steps

1. Parse the JSON project specs (they describe input files, compiler options, and expected outputs).
2. Compile the project with `ts.createProgram` using the spec's options.
3. Collect all emitted `.js` files.
4. Evaluate them in a single `quench-runtime::Context` with a CommonJS-style `require` stub that resolves modules within the emitted set.
5. Record pass/fail/skip.

## Boundaries

- Only add test harness code.
- Do not modify `tests/typescript/`.

## Acceptance criteria

- At least one CommonJS project spec runs end-to-end without crashing.
- AMD/System-only specs are skipped with a clear reason.

## Verification

```bash
cargo test -p quench-runtime --test project -- --nocapture
```
