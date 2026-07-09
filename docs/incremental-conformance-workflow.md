# Incremental Conformance Workflow

This document describes the workflow for driving Quench to 100% JS/TS
conformance one test file at a time. The harness is intentionally simple:
run tests in a deterministic order, stop at the first failure, convert that
failure into a focused regression test, fix it, and only then advance.

## Why incremental?

Running the entire test262 or TypeScript conformance suite at once produces a
wall of failures that is hard to act on. By processing files in a fixed order
and stopping at the first problem, every fix has a clear, reproducible target,
and regressions are caught immediately.

## Prerequisites

- `tests/test262/test/` submodule checked out.
- `tests/typescript/tests/cases/conformance/` submodule checked out (for TS).
- `cargo test -p quench-runtime` passes.

## Deterministic ordering

The harness collects test files with `test262::collect_test_files` and sorts
them lexicographically before execution. This guarantees that the "next"
failing file is the same on every run for a given codebase.

## Step-by-step workflow

1. **Pick a target area** from `docs/conformance-coverage-matrix.md` and add it
to the active subset in `crates/quench-runtime/tests/test262.rs` or
`crates/quench-runtime/tests/conformance.rs`.

2. **Run the incremental harness** for that subset:

   ```rust
   // In a test, script, or CLI helper:
   let report = quench_runtime::test262::run_suite_stop_on_fail(
       Path::new("tests/test262/test"),
       Some("language/statements/class"),
   )?;
   ```

   The harness stops at the first failing file.

3. **Create a regression test** from `crates/quench-runtime/tests/regression-template.rs`:
   - Copy the template to a new file.
   - Paste the minimal reproduction from the failing file.
   - Name the test after the spec area or file.

4. **Implement the smallest fix** that makes the regression test pass. Do not
fix unrelated issues in the same change.

5. **Verify**:

   ```bash
   cargo test -p quench-runtime --test regression_<name>
   cargo test -p quench-runtime
   ```

6. **Advance**: run the incremental harness again. It will now pass the
previous file and stop at the next failure.

7. **Repeat** until the active subset reports 100% pass / 0 spec skips.

8. **Update the matrix**: mark the area `- [x]` in
`docs/conformance-coverage-matrix.md` and update the task file status.

## Stop-on-fail API

All APIs are implemented and exported from `quench_runtime::test262`:

- `quench_runtime::test262::run_suite_stop_on_fail(root, subset)` — run files
  in deterministic order and stop at the first failure.
- `quench_runtime::test262::assert_test262_file_passes(path)` — unit-test
  helper to lock in a single file.
- `quench_runtime::test262::collect_test_files(dir)` — list files
  deterministically.

## Rules

- One behavior, one fix, one regression test.
- Do not skip a failing file unless the skip reason is recorded in a task file.
- Do not mark a conformance area complete until the incremental harness reports
  100% pass / 0 spec skips for it.
- Update `docs/conformance-coverage-matrix.md` and `tasks/index.json` as areas
  become active or complete.

## VM foundation

The conformance push is backed by an explicit-stack execution model. The following foundational *capabilities* are landed and tested; the corresponding architecture tasks remain open as improvements, not blockers:

- **Explicit-stack execution** — `shadow.rs` (SSTI) and `lower_hir.rs` / `eval_hir_source` (HIR) provide flat value/call stacks and prevent native-stack overflow. Task 85 (full trampoline interpreter) remains open for migrating the legacy recursive path.
- **NaN-boxed values + shapes** — `nanbox.rs`, `shape.rs`, and `Value::ObjectId(ObjectId)` are landed and exercised by SSTI tests. Task 335 (collapse all `Value` variants into `Value::Object` with `[[Call]]` / `[[Construct]]` slots) remains open.
- **Deterministic stop-on-fail harness** — `collect_test_files`, `run_suite_stop_on_fail`, and `assert_test262_file_passes` are exported from `quench_runtime::test262`.

New conformance fixes should be implemented as regression tests against the recursive interpreter first; SSTI/HIR parity for each language feature is handled under Task 264.

## Current active areas

See `docs/conformance-coverage-matrix.md` for the authoritative backlog.
The following areas have partial implementations and are candidates for
incremental harness pass:

- `tests/test262/test/language/statements/class` and
  `tests/test262/test/language/expressions/class` (Tasks 182/183/187)
- `tests/test262/test/built-ins/Error` and
  `tests/test262/test/built-ins/NativeErrors` (Task 250)
- `tests/test262/test/built-ins/Promise` (Task 251)
- VM infrastructure subsets that exercise `eval_shadow` / `eval_hir_source` (Task 264)
