# Task 42: Generate machine-readable conformance report

## Goal

Emit a JSON file with per-case results so CI, dashboards, and future trend tracking can consume conformance data programmatically.

## Pareto & reuse note

- Prefer existing crates, the Rust standard library, and OS features over custom code.
- Follow the 80/20 rule: include file path, status, category, and error message; skip large diffs initially.
- Defer edge cases, but document them in this task or spawn a follow-up task so they are not lost.

## TDD & testing note

- Follow the red-green-refactor cycle: write a failing unit test first, then the minimal code to pass it, then refactor.
- Add a regression test for every bug fix and edge case covered by this task.
- Keep tests in `crates/quench-runtime/tests/` and run `cargo test -p quench-runtime` before marking work done.

## Files

- `crates/quench-runtime/tests/conformance.rs`
- `crates/quench-runtime/Cargo.toml` (dev-dependency on `serde_json` if not already present)

## Report schema

```json
{
  "generated_at": "2026-06-30T12:00:00Z",
  "total": 200,
  "passed": 112,
  "failed": 46,
  "skipped": 42,
  "categories": {
    "classes": { "passed": 0, "failed": 46, "skipped": 0 }
  },
  "cases": [
    {
      "path": "tests/typescript/tests/cases/conformance/classes/classAbstractInstantiations2.ts",
      "status": "failed",
      "category": "classes",
      "error": "ReferenceError: C is not defined"
    }
  ]
}
```

## Steps

1. Define a serializable `ConformanceReport` struct.
2. After the harness finishes, write the report to `target/conformance-report.json` or `docs/conformance-report.json`.
3. Include summary counts, per-category counts, and per-case entries for failures and skips.
4. Add a test that verifies the report file is produced and contains valid JSON.

## Boundaries

- Only modify test harness code.
- Do not modify `tests/typescript/`.

## Acceptance criteria

- Running the harness produces a JSON report.
- The report can be parsed and contains the expected top-level fields.

## Timeout note

- All test commands must run with a timeout to avoid hangs from interpreter bugs or infinite loops.
- Use the `scripts/run_tests.sh` wrapper (if available) or prefix commands with `timeout 120` / `gtimeout 120`.
- In CI, set per-test and job-level timeouts (e.g., 5 minutes per test suite, 30 minutes per job).


## Verification

```bash
cargo test -p quench-runtime --test conformance -- --nocapture
ls target/conformance-report.json
```
