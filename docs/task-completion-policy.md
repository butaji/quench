> **Hard guard rails for marking tasks complete.** Applies to every task, including automated background processes.

# Task Completion Policy

## Rule 1: No task is complete without a regenerated conformance report

A task may be marked `COMPLETED` only when:

1. Its `target_subset` runs through the relevant harness (test262 or TypeScript conformance).
2. The harness output shows **100% pass rate with zero spec skips** for that subset.
3. The report file (`target/test262_report.json` / `target/conformance_report.json` or equivalent) is regenerated and matches the claimed result.
4. The regression test added for the task passes in parallel (`cargo test -p quench-runtime`).

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

## Rule 4: No status changes during active code drift

While a background process has uncommitted modifications in `crates/quench-runtime/src/`, no task affected by those files may be marked complete. The code must be committed and the harness must be re-run before status changes.

## Rule 5: Milestone tasks close only when their dependencies close

- Task 292 (var hoisting + TDZ milestone) closes only when Tasks 339 and 340 are closed.
- Task 296 (100% conformance) closes only when the full test262 + TypeScript suites pass.
- Batch-level exit criteria in `docs/js-ts-compatibility-roadmap.md` are the authoritative gates.

## Enforcement

- `scripts/target_tasks.py` exits with an error for any `COMPLETED` compatibility task (`suite` in `test262`, `typescript`, or `both`) that lacks a `## Verification` section or an `exit_criteria` field.
- The watchdog will revert any unauthorized `## Status: COMPLETED` changes made without evidence.
- The pre-existing pre-commit hook continues to enforce file/function limits; conformance evidence is in addition to those limits.
