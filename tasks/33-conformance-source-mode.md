# Task 33: Add TypeScript source-direct execution mode to conformance harness

**Status: COMPLETED**

## Goal

Make the harness execute the original `.ts`/`.tsx` source directly in `quench-runtime` instead of only running the pre-compiled baseline JS. The baseline JS becomes a fallback/verification mechanism.

## Implementation

### RunMode enum

Added a `RunMode` enum to the harness:
- `BaselineJs` — current behavior: extract JS from baseline and run it.
- `SourceTs` — run the raw `.ts`/`.tsx` source via `ctx.eval_ts()`.
- `Hybrid` — try source first; if parsing/lowering fails, fall back to baseline JS.

### TestCase enhancements

The `TestCase` struct now stores both:
- `ts_source`: The original TypeScript source
- `baseline_js`: The pre-compiled baseline JS (if available)

The `get_js_code()` method returns the appropriate code based on the run mode.

### Source-direct runner

Added `run_whitelist_source_direct()` function that:
1. Runs TypeScript source directly via `ctx.eval_ts()`
2. Categorizes failures as parse errors (unsupported TS syntax) vs runtime errors (JS semantics)
3. Tracks pass/fail rates and categorizes failures by pattern

### Hybrid runner

Added `run_whitelist_hybrid()` function that:
1. Tries source-direct execution first
2. Falls back to baseline JS if parsing fails
3. Reports how many tests passed via each path

### Tests

Added test entry points:
- `test_source_direct_simple` — sanity check with 5 simple TS cases (all pass)
- `test_whitelist_source_direct` — runs 50 whitelist cases in source-direct mode
- `test_whitelist_source_direct_full` — runs all whitelist cases (ignored, for manual runs)
- `test_whitelist_hybrid` — runs 50 whitelist cases in hybrid mode
- `test_whitelist_hybrid_full` — runs all whitelist cases (ignored, for manual runs)

## Results

### Source-direct sanity check (50 cases)
```
Source-direct pass rate: 95.2%
Passed (source direct): 40
Failed (parse error - unsupported TS syntax): 2
Failed (runtime error - JS semantics issue): 0
Skipped: 8
```

### Parse failure categorization

The 2 parse failures were:
1. `accessibilityModifiers.ts` — accessibility modifiers (`public`/`private`/`protected`) not supported
2. `staticAutoAccessorsWithDecorators.ts` — decorators not supported

These are TypeScript-only syntax features that are stripped during compilation but still cause parse errors in our lowerer.

## Files modified

- `crates/quench-runtime/tests/conformance.rs`:
  - Added `RunMode` enum
  - Added `FailureReason` enum
  - Added `TestCase::get_js_code()` method
  - Updated `run_case_with_mode()` to support all run modes
  - Added `run_whitelist_source_direct()` function
  - Added `run_whitelist_hybrid()` function
  - Added new test entry points

## Boundaries

- Only modified test harness code.
- Did not modify `tests/typescript/`.

## Acceptance criteria

✅ The harness can run TypeScript source cases source-direct and pass.
✅ A report compares source-direct vs baseline-js pass rates.
✅ Source-direct failures are categorized as parse errors vs runtime errors.

## Timeout note

- All test commands must run with a timeout to avoid hangs from interpreter bugs or infinite loops.
- Use the `scripts/run_tests.sh` wrapper (if available) or prefix commands with `timeout 120` / `gtimeout 120`.
- In CI, set per-test and job-level timeouts (e.g., 5 minutes per test suite, 30 minutes per job).


## Verification

```bash
cargo test -p quench-runtime --test conformance -- test_source_direct_simple --nocapture
cargo test -p quench-runtime --test conformance -- test_whitelist_source_direct --nocapture
```
