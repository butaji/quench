# Task 46: Add Quench harness for TypeScript compiler/ cases

## Goal

Run the ~6,500 regression cases in `tests/typescript/tests/cases/compiler/` by compiling them with TypeScript and executing the emitted JS in Quench.

## Pareto & reuse note

- Prefer existing crates, the Rust standard library, and OS features over custom code.
- Follow the 80/20 rule: start with a small sample (e.g., first 100 cases) and scale up as failures are fixed.
- Defer edge cases, but document them in this task or spawn a follow-up task so they are not lost.

## TDD & testing note

- Follow the red-green-refactor cycle: write a failing unit test first, then the minimal code to pass it, then refactor.
- Add a regression test for every bug fix and edge case covered by this task.
- Keep tests in `crates/quench-runtime/tests/` and run `cargo test -p quench-runtime` before marking work done.

## Files

- `tests/typescript/tests/cases/compiler/`
- `tests/typescript/tests/baselines/reference/`
- `crates/quench-runtime/tests/compiler.rs` (new)

## Steps

1. Walk `tests/typescript/tests/cases/compiler/**/*.ts`.
2. For each file:
   - Parse `// @target`, `// @module`, and other directives.
   - Compile with `ts.createProgram` or `ts.transpileModule` using the directives.
   - Extract the emitted JS.
   - Skip cases that produce type errors (unless the runtime should also error).
3. Run the emitted JS in `quench-runtime`.
4. Record pass/fail/skip and group failures by feature.
5. Add a test entry point `test_compiler_cases` with an optional sample limit.

## Boundaries

- Only add test harness code.
- Do not modify `tests/typescript/`.

## Acceptance criteria

- The harness runs over the `compiler/` directory without panicking.
- A summary report is produced (console + JSON after Task 42).

## Timeout note

- All test commands must run with a timeout to avoid hangs from interpreter bugs or infinite loops.
- Use the `scripts/run_tests.sh` wrapper (if available) or prefix commands with `timeout 120` / `gtimeout 120`.
- In CI, set per-test and job-level timeouts (e.g., 5 minutes per test suite, 30 minutes per job).


## Verification

```bash
cargo test -p quench-runtime --test compiler -- --nocapture
```
