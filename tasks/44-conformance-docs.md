# Task 44: Document the conformance harness architecture and workflow

## Goal

Write clear documentation for the TypeScript conformance harness so any contributor can run it, interpret results, and update baselines.

## Pareto & reuse note

- Prefer existing crates, the Rust standard library, and OS features over custom code.
- Follow the 80/20 rule: cover the common developer workflow first.
- Defer edge cases, but document them in this task or spawn a follow-up task so they are not lost.

## TDD & testing note

- Follow the red-green-refactor cycle: write a failing unit test first, then the minimal code to pass it, then refactor.
- Add a regression test for every bug fix and edge case covered by this task.
- Keep tests in `crates/quench-runtime/tests/` and run `cargo test -p quench-runtime` before marking work done.

## Files

- `docs/conformance.md` (new)
- `README.md`
- `EXECUTE.md`

## Document sections

1. **What the harness does**
   - Walks `tests/typescript/tests/cases/conformance/`.
   - Parses `// @name: value` directives.
   - Resolves the correct `.js` baseline (including configuration suffixes).
   - Handles single- and multi-file cases.
   - Executes the JS in `quench-runtime` and reports pass/fail/skip.
2. **How to run it**
   - `cargo test -p quench-runtime --test conformance -- --nocapture`
   - `cargo test -p quench-runtime --test conformance -- test_full_whitelist_conformance --nocapture`
3. **How to interpret the report**
   - Summary counts, category breakdown, JSON report location.
4. **How to add a new category to the whitelist**
   - Edit `WHITELIST_DIRS` in `conformance.rs`.
5. **How to validate with TypeScript's own runner**
   - Commands from Task 43.
6. **How to update baselines after a TypeScript submodule bump**
   - Re-run the reference runner and re-audit.

## Boundaries

- Only modify documentation files.
- Do not modify `tests/typescript/`.

## Acceptance criteria

- `docs/conformance.md` exists and a new contributor can follow it to run the harness.
- `README.md` links to `docs/conformance.md`.

## Verification

```bash
ls docs/conformance.md
```
