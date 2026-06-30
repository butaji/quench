# Task 32: Expand conformance harness to full whitelist run

**Status: COMPLETED**

## Goal

Remove the 200-case limit used in the Task 16 audit and run the conformance harness over every runtime-relevant case in the whitelist.

## Implementation

### Full whitelist test entry point

Added `test_full_whitelist_conformance` test function that:
- Runs ALL whitelist cases without any limit
- Prints progress every 100 cases
- Reports final pass/fail/skip counts
- Groups failures by category
- Shows top 20 failures

### Usage

The test is marked `#[ignore]` because it takes a long time to run. To execute:

```bash
cargo test -p quench-runtime --test conformance -- test_full_whitelist_conformance --nocapture
```

## Files modified

- `crates/quench-runtime/tests/conformance.rs`:
  - Added `test_full_whitelist_conformance()` function with full whitelist run logic

## Acceptance criteria

✅ `test_full_whitelist_conformance` entry point exists.
✅ Test runs all whitelist cases without a limit.
✅ Final report includes pass/fail/skip counts and failure categorization.

## Timeout note

- All test commands must run with a timeout to avoid hangs from interpreter bugs or infinite loops.
- Use the `scripts/run_tests.sh` wrapper (if available) or prefix commands with `timeout 120` / `gtimeout 120`.
- In CI, set per-test and job-level timeouts (e.g., 5 minutes per test suite, 30 minutes per job).


## Verification

```bash
cargo test -p quench-runtime --test conformance -- test_full_whitelist_conformance --nocapture
```
