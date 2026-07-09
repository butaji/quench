> **Preparation only — the harness runner itself is intentionally not implemented yet.**
> This task tracks the scaffolding, docs, and APIs needed for the incremental
> conformance workflow described in `docs/incremental-conformance-workflow.md`.

# Task 355: Incremental conformance harness prep

## Status: COMPLETED

## Goal

Provide the code, APIs, templates, and documentation required to implement an
incremental conformance harness that runs test files one at a time in
 deterministic order, stops at the first failure, and converts each failure into
a focused regression test before advancing.

## Exact preparation

1. Deterministic ordering
   - `test262::collect_test_files` returns files sorted lexicographically.
   - `test262::run_suite` sorts files before processing.

2. Stop-on-fail primitive
   - `test262::run_suite_stop_on_fail(root, subset)` runs files in order and
     stops at the first `TestOutcome::Fail`.

3. Single-file assertion helper
   - `test262::assert_test262_file_passes(path)` for unit-test guards.

4. Regression test template
   - `crates/quench-runtime/tests/regression-template.rs` is copy-paste ready
     for converting a failing file into a Rust regression test.

5. Documentation
   - `docs/incremental-conformance-workflow.md` defines the step-by-step process,
     rules, and API usage.

## Acceptance criteria

- [x] Test file collection is deterministic (`test262::collect_test_files`).
- [x] `run_suite_stop_on_fail` exists, is exported, and stops at the first `Fail`.
- [x] `assert_test262_file_passes` exists, is exported, and panics on non-pass.
- [x] Regression template compiles and its example test passes.
- [x] Workflow documentation is in place.
- [x] Harness APIs are exercised by the existing test binaries.

## Notes

The automated loop that repeatedly runs the harness, creates regression tests,
and applies fixes is intentionally out of scope. All building blocks for that
loop are now in place and exported from `quench_runtime::test262`.

Restored after accidental removal: `collect_test_files`, `run_suite_stop_on_fail`,
and `assert_test262_file_passes` are exported from `quench_runtime::test262` and
used by `docs/incremental-conformance-workflow.md`.

The VM foundation (Task 85 closed, Task 264 in progress, Task 335 in progress)
now provides the explicit-stack execution paths needed to run large conformance
subsets reliably. Future conformance tasks should use the incremental workflow
defined in `docs/incremental-conformance-workflow.md` and add regression tests
via the template before advancing to the next failing file.

## Targets

- **Suite:** harness
- **Batch:** 0
- **Target subset:** n/a (tooling task)
- **Blocked by:** none
- **Exit criteria:** All preparation items above are merged and documented.
