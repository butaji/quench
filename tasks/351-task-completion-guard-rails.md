> **Hard policy: no task can be marked complete without 100% conformance evidence.**

# Task 351: Enforce task completion guard rails

## Status: PENDING

## Goal

Prevent premature or automated completion claims by requiring regenerated harness reports before any task status changes to `COMPLETED`.

## Exact implementation

1. Create and keep current `docs/task-completion-policy.md` with the five rules:
   - Rule 1: 100% pass / zero skips in `target_subset`.
   - Rule 2: Background processes cannot self-certify completion.
   - Rule 3: Every task must have `## Verification` and `exit_criteria`.
   - Rule 4: No status changes during uncommitted code drift.
   - Rule 5: Milestone tasks close only when dependencies close.
2. Reference the policy from `docs/conformance.md`.
3. As watchdog, before any task status is updated to `COMPLETED`, verify:
   - The task file contains `## Verification` and `exit_criteria`.
   - The background process has committed the relevant code changes.
   - A harness report regenerated after the commit shows 100% pass / 0 skips for the task's `target_subset`.
   - Regression tests pass in parallel (`cargo test -p quench-runtime`).
4. Revert any unauthorized `## Status: COMPLETED` change that lacks evidence and reopen the task.

## Acceptance criteria

- [ ] `docs/task-completion-policy.md` exists and is referenced by `docs/conformance.md`.
- [ ] Every `COMPLETED` compatibility task in `tasks/index.json` has a non-empty `exit_criteria` and a `## Verification` section.
- [ ] No task is marked `COMPLETED` while its `target_subset` still shows failures or skips.

## Targets

- **Suite:** `tooling`
- **Batch:** 0
- **Target subset:** n/a (process enforcement)
- **Blocked by:** none
- **Exit criteria:** Policy doc is in place and all future status changes follow it.

## Verification

Run the task targeting script and confirm no completed compatibility task is
missing a `## Verification` section or an `exit_criteria` field:

```bash
python3 scripts/target_tasks.py
python3 -m json.tool tasks/index.json > /dev/null
```
