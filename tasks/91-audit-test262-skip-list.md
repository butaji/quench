# Task 91: Audit and shrink test262 feature skip list

## Status: PENDING

## Gap

The test262 runner maintains a feature skip list that may be hiding real failures or skipping tests for features that are now implemented. Outdated skips reduce the signal from conformance runs.

## Fix

- Review the current feature skip list in the test262 runner/harness.
- Remove skips for features that are now supported.
- Re-run the conformance subset and file new tasks for any newly surfaced failures.

## Acceptance criteria

- [ ] Every remaining skip has a documented reason and a linked task.
- [ ] At least one outdated skip is removed and the corresponding tests pass.
- [ ] Conformance report is regenerated and shows improved true coverage.
- [ ] The harness is one step closer to running the full test262 suite with zero unjustified skips.

## Guardrail

Skips hide the true compatibility percentage. This task is not done until outdated skips are gone and only intentional deferrals remain, each linked to `docs/deferrals.md` or an open task.

## Files

- `crates/quench-runtime/src/test262/runner.rs`
- `crates/quench-runtime/src/test262/harness.rs`
- `target/conformance_report.md`

## Dependencies

- Task 253 (real harness includes loaded)

## Verification

```bash
cargo test -p quench-runtime --test test262 -- --nocapture
```

## Targets

- **Suite:** `test262`
- **Batch:** 0
- **Target subset:** `target/test262_report.md` accuracy — every remaining skip is justified or removed.
- **Blocked by:** see `tasks/index.json`
- **Exit criteria:** Outdated test262 feature skips are removed, only intentional deferrals remain, and the harness report is regenerated showing improved true coverage.
