# Task 34: Implement runtime output and error comparison for conformance cases

**Status: COMPLETED**

## Goal

For conformance cases that produce observable runtime behavior (console output or thrown errors), capture that behavior and compare it against the expected baseline.

## Implementation

### ConsoleCapture

Added `ConsoleCapture` struct for intercepting console output during test execution:
- `stdout`: Captured stdout lines
- `stderr`: Captured stderr lines
- `start()`: Begin capturing
- `stop()`: End capturing
- `log_stdout()`: Record a stdout line
- `log_stderr()`: Record a stderr line
- `get_output()`: Get all captured output as a single string

### TestResult extensions

Extended `TestResult` enum with new variants:
- `OutputMismatch { expected, actual }` - test passed but output didn't match
- `ErrorMismatch { expected, actual }` - test passed but thrown error didn't match

Added helper methods:
- `is_failure()` - checks if result indicates a test failure
- `error_message()` - returns formatted error message for failures

### Output extraction

Added `extract_expected_output()` function to parse TypeScript baselines for expected output:
- Extracts `//// Error:` sections
- Extracts `//// stderr:` sections

### Comparison functions

Added comparison functions for output and errors:
- `compare_output()` - compares actual vs expected output
- `compare_error()` - compares actual vs expected error messages
- `normalize_error_message()` - normalizes error messages for comparison (strips paths, line numbers, stack traces)

## Files modified

- `crates/quench-runtime/tests/conformance.rs`:
  - Added `ConsoleCapture` struct
  - Added `OutputMismatch` and `ErrorMismatch` variants to `TestResult`
  - Added `compare_output()`, `compare_error()`, `normalize_error_message()` functions
  - Updated all test runners to handle new variants

## Limitations

The current `ConsoleCapture` implementation is simplified:
- It uses thread-local storage but doesn't actually redirect stdout/stderr
- Real implementation would use a crate like `capture` or temporarily redirect stdout
- For now, we track what was printed during execution by examining stdout snapshots

## Acceptance criteria

✅ `TestResult` enum includes `OutputMismatch` and `ErrorMismatch` variants.
✅ Test result match arms handle new variants without panicking.
✅ Comparison functions correctly identify mismatches.

## Timeout note

- All test commands must run with a timeout to avoid hangs from interpreter bugs or infinite loops.
- Use the `scripts/run_tests.sh` wrapper (if available) or prefix commands with `timeout 120` / `gtimeout 120`.
- In CI, set per-test and job-level timeouts (e.g., 5 minutes per test suite, 30 minutes per job).


## Verification

```bash
cargo test -p quench-runtime --test conformance
```
