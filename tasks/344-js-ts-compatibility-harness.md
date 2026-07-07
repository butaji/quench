> The canonical harness that runs the test262 and TypeScript submodule suites and reports the true JS/TS/TSX/JSX compatibility percentage.

# Task 344: JS/TS Compatibility Harness

## Status: PENDING

## Goal

Provide a single, repeatable way to run the external spec suites from `tests/test262/` and `tests/typescript/`, produce a compatibility percentage, and expose the gap to 100%.

The harness is the top of the testing pyramid (see `docs/testing-strategy.md`). It does not replace fast Rust unit tests or spec fixtures; it validates that the runtime really matches ECMA-262 and the TypeScript language spec at scale.

## What the harness does

1. Discovers every test case in the initialized submodules.
2. Runs each case in an isolated `Context` (and, where needed, an isolated thread).
3. Loads real test262 harness includes (`assert.js`, `sta.js`, etc.) from `tests/test262/harness/` with Rust fallbacks for unsupported helpers.
4. Records `passed`, `failed`, and `skipped` per suite and per feature/category.
5. Writes JSON and Markdown reports to `target/test262_report.json` / `target/test262_report.md` and `target/conformance_report.json` / `target/conformance_report.md`.

## Running the harness

Initialize submodules once:

```bash
git submodule update --init tests/test262 tests/typescript
```

Run the full harness and see the compatibility percentage:

```bash
# test262
cargo test -p quench-runtime --test test262 -- --ignored --nocapture

# TypeScript conformance
cargo test -p quench-runtime --test conformance -- --test-threads=1

# Or use the wrapper script
./scripts/run_tests.sh test-test262
./scripts/run_tests.sh test-conformance
```

Reports are regenerated after each run in `target/`.

## Current baseline

| Suite | Full spec size | Current subset | Pass rate (subset) | True spec coverage |
|-------|---------------|----------------|--------------------|--------------------|
| test262 | ~53,683 `.js` files | 431 | 10.9% | 0.09% |
| TypeScript conformance | ~18,876 cases | 376 | 40.7% | 0.81% |

## Target

**100% of the full test262 suite and 100% of the full TypeScript conformance suite pass with zero spec skips.**

See `docs/conformance.md` for the detailed harness design and `tasks/296-100-percent-js-ts-compatibility.md` for the north-star goal.

## Acceptance criteria

- [ ] A single documented command runs the test262 harness and prints the pass rate.
- [ ] A single documented command runs the TypeScript conformance harness and prints the pass rate.
- [ ] Both harnesses produce machine-readable JSON reports and human-readable Markdown reports.
- [ ] Reports show pass/fail/skip counts and per-feature/category breakdowns.
- [ ] The harness runs to completion without process aborts on the current subset.
- [ ] `docs/conformance.md` is kept up to date with the latest commands and baseline numbers.
- [ ] The harness can expand from the current subset to the full suites as runtime support grows, eventually covering all ~53,683 test262 files and ~18,876 TypeScript cases.

## Guardrails

- **No task may claim 100% compatibility without a harness report proving it.** `target/test262_report.md` and `target/conformance_report.md` are the evidence.
- **Skips must be justified.** Every skipped test must link to a deferral in `docs/deferrals.md` or an open task. Unsupported features must fail, not hide.
- **Reports are regenerated before any milestone is closed.** The numbers in the task file must match the latest report.
- **The harness is the final gate.** Fast unit tests and spec fixtures catch regressions quickly, but the harness decides when an area is complete.

## Dependencies

- Task 253 (real test262 harness includes loaded)
- Task 82 (whole-suite conformance analysis)
- Task 91 (audit test262 skip list)
- Task 250 (preserve thrown values for negative tests)

## Verification

```bash
./scripts/run_tests.sh test-test262
./scripts/run_tests.sh test-conformance
cat target/test262_report.md
cat target/conformance_report.md
```

## Targets

- **Suite:** `both`
- **Batch:** 0
- **Target subset:** `tests/test262/` + `tests/typescript/tests/cases/conformance/` harness integration.
- **Blocked by:** 253, 91, 250, 82
- **Exit criteria:** A single documented command runs each suite, produces JSON and Markdown reports, and reports truthful pass/fail/skip counts with zero helper-stub skips.
