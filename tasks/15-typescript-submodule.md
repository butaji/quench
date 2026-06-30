# Task 15: Add TypeScript conformance submodule and test harness

## Goal

Bring the official TypeScript test corpus into the repo and build a runner that can compile and execute conformance cases in `quench-runtime`.

## Files

- `.gitmodules`
- `tests/typescript/` (shallow submodule)
- `crates/quench-runtime/tests/conformance.rs` (new test harness)
- `crates/quench-runtime/Cargo.toml` (if new dev-dependencies are needed)

## Steps

1. Add the TypeScript repo as a shallow submodule:
   ```bash
   git submodule add --depth 1 https://github.com/microsoft/TypeScript.git tests/typescript
   ```
2. Commit `.gitmodules` and the submodule pointer.
3. Create `crates/quench-runtime/tests/conformance.rs` with a harness that:
   - Walks `tests/typescript/tests/cases/conformance/**/*.ts`.
   - Filters out type-check-only files (e.g., files whose only assertions are `// @errors`) or keeps them and expects zero runtime errors.
   - Compiles each `.ts` file to JS. Prefer using the existing `swc` infrastructure in `quench-runtime`; fall back to invoking `tsc` if swc cannot handle a file.
   - Evaluates the emitted JS in a fresh `quench_runtime::Context`.
   - Captures runtime errors and console output.
   - Compares against baseline output in `tests/typescript/tests/baselines/reference/` when available.
4. Add a single sanity test that compiles and runs one trivial conformance file (e.g., `tests/cases/conformance/expressions/additionOperator/additionOperatorWithNumberAndDate.ts` if it exists) to prove the harness works.
5. Document how to initialize the submodule in `EXECUTE.md` and `README.md`.

## Boundaries

- Do not modify files inside `tests/typescript/`.
- Do not change runtime behavior in this task; the harness may report failures.
- `examples/` remain immutable.

## Acceptance criteria

- `git submodule update --init tests/typescript` succeeds and the directory is populated.
- `cargo test -p quench-runtime --test conformance` discovers files and runs at least one sanity case.
- The harness prints a summary of passed/failed/skipped cases.

## Verification

```bash
git submodule update --init tests/typescript
cargo test -p quench-runtime --test conformance
```
