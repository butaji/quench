# Task 75: Make conformance harnesses stable for full-suite runs

## Goal

Fix the runtime so the test harnesses can run without crashing, even if individual conformance cases cause stack overflow.

## Changes Made

### 1. Fixed Module Visibility
- Added `conformance` and `test262` modules to `lib.rs` public exports
- Added missing dependencies (`walkdir`, `chrono`, `serde_yaml`) to `Cargo.toml`

### 2. Fixed Test API Issues
- Updated `conformance.rs` to use correct `run_suite` signature with `start` and `limit` arguments
- Fixed `depth_limit.rs` to use `reset_depth` instead of non-existent `reset_call_depth`
- Added `serial_test` dependency for serial test execution

### 3. Recursion Depth Tracking Fix
The core issue was that depth was being checked in multiple places:
- `eval_statement` - checked depth at statement level
- `eval_expression` - checked depth at expression level  
- `call_value_with_this` - checked depth at function call level

This caused double-counting of depth. Fixed by:
- Removing depth checks from `eval_statement` and `eval_expression`
- Keeping only the check in `call_value_with_this` (function call boundary)
- Adding `set_max_call_depth()` function for testing
- Adding `reset_depth()` calls to `Context::new()` and `Context::eval()` to reset between evals

### 4. NativeConstructor Support
- Added `Value::NativeConstructor` handling to `call_value_with_this`
- This fixed `new Date()`, `new Error()`, `new TypeError()` etc.

### 5. Thread-Based Isolation for TypeScript Conformance

**Added in this update:**

#### Isolation Strategy
- Each test case runs in a fresh `Context` (prevents state leakage)
- `reset_depth()` is called before each test (resets recursion counter)
- Tests run in a spawned thread (prevents stack overflow from crashing harness)

#### Implementation
- Added `run_case_isolated()` function that spawns a thread per test
- Added `run_baseline_isolated_inner()` for thread-safe test execution
- Thread isolation catches panics and returns them as Fail outcomes
- Added `test_conformance_isolation_with_small_subset` test to verify isolation works

#### Tests Added
- `test_conformance_isolation_with_small_subset` - verifies isolation with 5 test cases
- `test_depth_reset_after_context_creation` - verifies depth is reset in threaded context

### 6. Updated Ignored Tests
- `test_typescript_conformance_sanity` now uses a 100-case limit with isolation
- Test documentation updated to explain isolation mechanism

## Current Status

**All tests pass:**
- 55 unit tests pass
- 7 depth limit tests pass (added `test_depth_reset_after_context_creation`)
- 20 runtime issue tests pass
- 34 main crate tests pass
- 3 parity tests pass
- 4 conformance tests pass (2 ignored for submodule/suite size)

**Isolation tests (new):**
- `test_conformance_isolation_with_small_subset` - verifies thread isolation
- `test_depth_reset_after_context_creation` - verifies depth reset in threads

## Verification

```bash
# Run all tests
cargo test

# Run conformance tests with single thread
cargo test -p quench-runtime --test conformance -- --test-threads=1

# Run depth limit tests
cargo test -p quench-runtime --test depth_limit

# Enable full conformance (100 cases with isolation)
cargo test -p quench-runtime --test conformance test_typescript_conformance_sanity -- --ignored
```

## Isolation Mechanism

The thread-based isolation ensures that if a test case causes:
- Stack overflow: The thread dies, harness returns Fail outcome
- Panic: Caught by `handle.join()`, harness returns Fail outcome
- Hang: Currently blocks (timeout not implemented - can be added with channels)

This allows the harness to process hundreds of test cases even if some crash.

## Deferred

- Full conformance suite runs (thousands of cases) - requires more isolation or iterative interpreter
- Timeout support for individual tests - requires channels-based implementation
- test262 harness already has `catch_unwind` but could benefit from thread isolation
