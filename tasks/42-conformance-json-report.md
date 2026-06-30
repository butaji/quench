# Task 42: Generate machine-readable conformance report

## Status: COMPLETED

### What was done (2026-06-30)

Added `CaseResult` and `ConformanceReport` structs to `conformance.rs` with serde derives:

```rust
#[derive(Debug, serde::Serialize, serde::Deserialize)]
struct CaseResult {
    name: String,       // relative path from repo root
    category: String,   // conformance sub-category
    status: String,     // "pass" | "fail" | "skip"
    message: Option<String>,  // error message if failed
}

#[derive(Debug, serde::Serialize, serde::Deserialize)]
struct ConformanceReport {
    timestamp: String,  // ISO-8601
    total: usize,
    passed: usize,
    failed: usize,
    skipped: usize,
    pass_rate: f64,     // percentage, 1 decimal
    cases: Vec<CaseResult>,
}
```

The report is written to `target/conformance_report.json` by `write_json_report()` and integrated into `run_whitelist_source_direct()`.

Added `chrono` to `dev-dependencies` for ISO-8601 timestamps.

### Example output

```json
{
  "timestamp": "2026-06-30T23:45:00Z",
  "total": 200,
  "passed": 112,
  "failed": 46,
  "skipped": 42,
  "pass_rate": 70.9,
  "cases": [
    {
      "name": "es6/for-of1.ts",
      "category": "es6",
      "status": "pass",
      "message": null
    },
    {
      "name": "classes/classDecl.ts",
      "category": "classes",
      "status": "fail",
      "message": "ReferenceError: undefined is not a function"
    }
  ]
}
```

### Files changed

- `crates/quench-runtime/tests/conformance.rs` — structs, `write_json_report()`, `case_to_result()`
- `crates/quench-runtime/Cargo.toml` — added `chrono` dev-dependency
- `crates/quench-runtime/tests/conformance.rs` — 2 new tests

### Verification

```bash
cargo test -p quench-runtime --test conformance test_json_report  # ✓
```

### Remaining work

- Parse the JSON report in CI to enforce pass-rate threshold as a hard gate
- Add trending: compare with previous report to detect regressions
