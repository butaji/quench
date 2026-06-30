# Task 31: Add test timeout protection

## Status: ✅ COMPLETED

## Summary

Implemented comprehensive timeout protection for all tests with multiple layers of defense:

### 1. System-level timeout (xtask)
- Created `xtask/` crate that wraps `cargo test` with the `timeout` command
- Default timeout: 300 seconds (5 minutes)
- Short timeout: 60 seconds  
- Long timeout: 600 seconds (for conformance tests)
- Works on both Linux (native `timeout`) and macOS (`gtimeout` from coreutils)

### 2. Per-test timeout (for runtime tests)
- Added `with_timeout()` function in `runtime_tests.rs` that monitors test execution
- Uses a monitoring thread that sets a flag after timeout expires
- If a test exceeds its timeout, it's marked as failed rather than hanging
- Default per-test timeout: 60 seconds

### 3. Binary execution timeout (for parity tests)
- The `parity.rs` tests use thread-based timeout for spawning binary processes
- Default: 60 seconds per binary execution

## Usage

```bash
# Run all tests with 5min timeout
./scripts/run_tests.sh

# Run specific test with timeout
./scripts/run_tests.sh test_runtime_loads

# Run with Make
make test

# Or use xtask directly
cargo run -p xtask -- test
```

## Files Changed

- `xtask/` - New xtask crate for timeout-protected commands
- `scripts/run_tests.sh` - Wrapper using xtask
- `Makefile` - Targets with timeout
- `crates/quench-runtime/tests/runtime_tests.rs` - Per-test timeout
- `tests/parity.rs` - Binary execution timeout

## Notes

- If `timeout`/`gtimeout` is not installed, tests will run without system-level timeout
- macOS: `brew install coreutils` to get `gtimeout`
- Linux: `timeout` is usually available by default
