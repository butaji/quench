> **Authoritative backlog: every spec area that must pass for 100% compat.**

# Task 352: Maintain the conformance coverage matrix

## Status: PENDING

## Goal

Make it impossible for the background process (or any worker) to mark a compatibility task complete without actually conquering a concrete, enumerable spec area. The matrix in `docs/conformance-coverage-matrix.md` lists every test262 and TypeScript conformance directory that must reach 100% pass / 0 spec skips.

## Exact implementation

1. Keep `docs/conformance-coverage-matrix.md` up to date as the source of truth.
2. Every compatibility task (`suite` in `test262`, `typescript`, or `both`) must name one or more matrix paths in its `target_subset`.
3. Before a compatibility task moves to `COMPLETED`, verify:
   - The area is in the active harness subset (`crates/quench-runtime/tests/test262.rs` or `crates/quench-runtime/tests/conformance.rs`).
   - A regenerated report shows 100% pass and zero spec skips for that area.
   - The matrix checkbox for that area is flipped to `- [x]`.
4. Reject vague targets such as "expressions subset" or "built-ins coverage"; insist on exact paths.

## Acceptance criteria

- [ ] `docs/conformance-coverage-matrix.md` exists and lists all test262 + TypeScript conformance directories with file counts.
- [ ] `docs/task-completion-policy.md` references the matrix as the source of truth.
- [ ] `docs/js-ts-compatibility-roadmap.md` requires the active subset to grow until it matches the matrix.
- [ ] No `COMPLETED` compatibility task has a generic `target_subset`.

## Targets

- **Suite:** `tooling`
- **Batch:** 0
- **Target subset:** n/a (process enforcement)
- **Blocked by:** 351
- **Exit criteria:** Matrix is authoritative, actively maintained, and all closed compat tasks cite concrete matrix paths.

## Verification

Confirm the matrix and policy are consistent and the targeting script still passes:

```bash
python3 scripts/target_tasks.py
python3 -m json.tool tasks/index.json > /dev/null
```
