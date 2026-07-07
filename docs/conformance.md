> Running the test262 and TypeScript conformance harnesses.

# Conformance

External submodules:

```bash
git submodule update --init tests/test262 tests/typescript
```

Run:

```bash
cargo test -p quench-runtime --test test262 -- --ignored --nocapture
cargo test -p quench-runtime --test conformance -- --test-threads=1
```

## Full spec size vs. current subset

| Suite | Total files/cases in spec | Current subset | % of spec exercised |
|---|---|---|---|
| test262 | ~53,683 `.js` files | 431 | **0.8%** |
| TypeScript conformance | ~18,876 `.ts` cases | 376 | **2.0%** |

The subsets are intentionally small so the harness runs in seconds while the runtime is still incomplete. The **target is 100% of the full suites**: all ~53,683 test262 files and all ~18,876 TypeScript conformance cases. The subset grows as features land.

## Latest results on the current subset

| Suite | Subset total | Passed | Failed | Skipped | Pass rate (of subset) | Pass rate (of non-skipped) |
|---|---|---|---|---|---|---|
| TypeScript expressions | 376 | 153 | 223 | 0 | **40.7%** | **40.7%** |
| test262 | 431 | 47 | 86 | 298 | **10.9%** | **35.3%** |

## True spec coverage

| Suite | Passed / total spec files | True coverage |
|---|---|---|
| test262 | 47 / 53,683 | **0.09%** |
| TypeScript | 153 / 18,876 | **0.81%** |

The TypeScript harness runs baseline JS extracted from compiler output, not source TS. The test262 harness now loads real harness includes but still skips tests that require unsupported features or includes that are not yet implemented.

Do not edit `tests/test262/`, `tests/typescript/`, or `examples/`.

## Targeting policy

Every compatibility task must be targeted at a measurable subset of the spec suites. The fields below are required in `tasks/index.json` and are maintained by `scripts/target_tasks.py`:

| Field | Allowed values | Purpose |
|-------|----------------|---------|
| `suite` | `test262`, `typescript`, `both`, `harness`, `runtime`, `tooling` | Primary suite or work type the task advances. |
| `category` | `harness`, `measurement`, `expressions`, `statements`, `functions`, `classes`, `built-ins`, `objects`, `errors`, `modules`, `async`, `types`, `jsx`, `interpreter`, `testing`, `refactor` | Semantic area for batching and reporting. |
| `batch` | integer | Work batch from the roadmap; lower numbers run first. |
| `target_subset` | path or pattern | Concrete location in `tests/test262` or `tests/typescript` that the task must bring to 100%. |
| `blocked_by` | list of task IDs | Tasks that must close before this one can realistically close. |
| `exit_criteria` | sentence | Verifiable 100% pass condition, e.g. "test262 language/expressions/ subset passes at 100% with zero spec skips." |

No compatibility task may be marked complete without a regenerated harness report proving its `target_subset` is at 100% with zero spec skips. The batch taxonomy is in `docs/js-ts-compatibility-roadmap.md`.
