# Task 31: Add test timeout protection

**Status: COMPLETED** - Timeout wrapper and test runner script created.

## Goal

Ensure tests have timeout protection to prevent them from hanging indefinitely.

## Solution

Created a test wrapper script that uses the system `timeout` command:

```bash
# Run all tests with 5-minute timeout
./scripts/run_tests.sh

# Run specific test with timeout
./scripts/run_tests.sh test_runtime_loads
```

## Files Created

- `scripts/run_tests.sh` - Test runner with timeout wrapper
- `crates/quench-runtime/tests/test_harness.rs` - Documentation and helper types for timeout handling

## Timeout Strategy

1. **System-level timeout** (recommended): Uses the `timeout` command from GNU coreutils
2. **Cargo test threads**: Use `--test-threads=1` to prevent parallel test interference

## Test Categories

| Category | Expected Duration | Timeout |
|----------|------------------|---------|
| Unit tests | < 5s | 30s |
| Integration tests | ~30-60s | 120s |
| Conformance tests | ~120s | 300s |

## Usage

```bash
# Install timeout on macOS
brew install coreutils

# Run all tests with timeout
./scripts/run_tests.sh

# Run tests without timeout (if timeout command not available)
cargo test

# Run tests serially (recommended for debugging)
cargo test -- --test-threads=1
```

## Notes

- The `Context` type is not `Send`, so per-test timeout wrappers cannot move tests to separate threads
- The system-level timeout is the most reliable approach
- Tests that are known to hang (like full counter.js loading) are marked with `#[ignore]`
