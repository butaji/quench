> **Hard guard rails for marking tasks complete.** Applies to every task, including automated background processes.

# Task Completion Policy

## Rule 1: No task is complete without a regenerated conformance report

A task may be marked `COMPLETED` only when:

1. Its `target_subset` is one or more concrete areas listed in `docs/conformance-coverage-matrix.md`.
2. The area is added to the active harness subset in `crates/quench-runtime/tests/test262.rs` or `crates/quench-runtime/tests/conformance.rs`.
3. The harness output shows **100% pass rate with zero spec skips** for that subset.
4. The report file (`target/test262_report.json` / `target/conformance_report.json` or equivalent) is regenerated and matches the claimed result.
5. The regression test added for the task passes in parallel (`cargo test -p quench-runtime`).

## Rule 2: Background processes may not self-certify completion

- A background process may implement code, add tests, and run commands.
- A background process **may not** change a task's `## Status:` line to `COMPLETED`.
- Only a human review or the watchdog process may update status after verifying the evidence in Rule 1.

## Rule 3: Required evidence in every compatibility task file

Every task whose `suite` is `test262`, `typescript`, or `both` must contain:

- A `## Verification` section with the exact command(s) to run.
- An `## Exit criteria` sentence that names the exact subset and the 100% / zero-skip condition.
- A link or path to the regenerated report, if applicable.

If any of these sections are missing, the task stays `PENDING` regardless of code state.

The `target_subset` and `exit_criteria` must name exact paths from `docs/conformance-coverage-matrix.md`, not broad categories like "expressions subset".

## Rule 4: No status changes during active code drift

While a background process has uncommitted modifications in `crates/quench-runtime/src/`, no task affected by those files may be marked complete. The code must be committed and the harness must be re-run before status changes.

## Rule 5: Milestone tasks close only when their dependencies close

- Task 292 (var hoisting + TDZ milestone) closes only when Tasks 339 and 340 are closed.
- Task 296 (100% conformance) closes only when the full test262 + TypeScript suites pass.
- Batch-level exit criteria in `docs/js-ts-compatibility-roadmap.md` are the authoritative gates.

## Rule 6: The coverage matrix is the source of truth

- `docs/conformance-coverage-matrix.md` lists every spec area that must pass for 100% compatibility.
- A compatibility task must move at least one matrix area to `- [x] Active & passing`.
- Background processes must update the matrix as part of the task evidence; a task is not complete if the matrix still shows the area as `- [ ]` or `- [/]`.
- Task 296 closes only when every entry in the matrix is `- [x]`.

## Enforcement

- `scripts/target_tasks.py` exits with an error for any `COMPLETED` compatibility task (`suite` in `test262`, `typescript`, or `both`) that lacks a `## Verification` section or an `exit_criteria` field.
- The watchdog will review the `target_subset` of every closed compat task against `docs/conformance-coverage-matrix.md`; vague targets like "expressions subset" are rejected.
- The watchdog will revert any unauthorized `## Status: COMPLETED` changes made without evidence.
- The pre-existing pre-commit hook continues to enforce file/function limits; conformance evidence is in addition to those limits.
