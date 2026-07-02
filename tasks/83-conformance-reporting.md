> **Improves harness output so failures are actionable and compatibility percentage is obvious.**

# Task 83: Improve conformance harness reporting

## Goal

Make the test262 and TypeScript conformance harnesses report results in a way that makes it easy to find the next bug to fix and track the overall compatibility percentage.

## Current state

Both harnesses already produce JSON reports and a basic summary. This task adds:

- A printed pass rate.
- Grouping of failures by error signature.
- Per-category pass/fail breakdown.
- A human-readable Markdown report alongside the JSON report.

## Files

- `crates/quench-runtime/src/conformance/report.rs`
- `crates/quench-runtime/src/test262/runner.rs`
- `crates/quench-runtime/tests/conformance.rs`
- `crates/quench-runtime/tests/test262.rs`
- `docs/conformance.md`

## Changes made

1. **`Report::print_summary`** now prints:
   - Total / passed / failed / skipped counts.
   - Overall pass rate.
   - Top 10 failure signatures with counts and example paths.
   - Top 10 categories by failure count with per-category pass rates.

2. **`Report::write_markdown`** writes `target/conformance_report.md` and `target/conformance_expressions_report.md`.

3. **`Test262Report::print_summary`**, **`Test262Report::write_markdown`**, and **`write_report`** now do the same for test262.

4. Test entry files now call both `write_json` and `write_markdown` after a run.

5. test262 reports are written to the project `target/` directory (same location as TypeScript reports).

## Verification

```bash
# TypeScript expressions subset
cargo test -p quench-runtime --test conformance test_typescript_conformance_expressions -- --ignored --nocapture
# -> prints pass rate, top errors, category breakdown

# test262 subset
cargo test -p quench-runtime --test test262 test262_expressions -- --ignored --nocapture
# -> prints pass rate, top errors, category breakdown
```

After running, open:

- `target/conformance_expressions_report.md`
- `target/test262_report.md`

## Status

`completed`. Future harness work (whole-suite stability) is tracked in Task 82.
