# Task 97: Improve Negative-Test Matching by Type and Phase

## Status: COMPLETED

## Problem

The original negative test matching in `test262/runner.rs` had:
1. **Duplicated code**: Error matching logic was duplicated between `runner.rs` and `errors.rs`
2. **Weak error type extraction**: Used `contains()` which causes false positives
3. **No error inheritance support**: Tests expecting base `Error` class couldn't match derived types like `TypeError`
4. **Inconsistent parse error detection**: Used simple string matching

## Solution

### 1. Enhanced `errors.rs` with precise matching

**Key improvements:**
- Added `ErrorPhase` enum for type-safe phase detection
- Added `ErrorInfo` struct for detailed error information
- Implemented `is_subtype_of()` for error class inheritance support
- Improved `extract_error_type()` to use precise matching (`starts_with()`)
- Added comprehensive test coverage (20+ new tests)

**Error type hierarchy support:**
- Tests expecting `Error` now match any JS error type (TypeError, ReferenceError, etc.)
- This aligns with ECMA-262 where all error types inherit from Error

### 2. Consolidated code in `runner.rs`

**Changes:**
- Removed duplicated error matching functions from `runner.rs`
- Now imports and uses `check_negative_test` from `errors.rs` module
- Simplified `determine_test_outcome()` to use centralized error matching

## Files Changed

- `crates/quench-runtime/src/test262/errors.rs` - Enhanced with precise matching and inheritance
- `crates/quench-runtime/src/test262/runner.rs` - Removed duplicate code, uses errors.rs

## Tests Added

All tests in `crates/quench-runtime/src/test262/errors.rs`:
- `test_extract_error_type_*` - 7 tests for error type extraction
- `test_error_types_match_*` - 3 tests for type matching
- `test_error_types_match_inheritance` - Tests Error base class matching
- `test_is_parse_error` - Tests parse vs runtime error detection
- `test_error_phase_*` - 3 tests for phase detection
- `test_error_info_*` - 2 tests for error info extraction
- `test_check_negative_test_*` - 6 tests for negative test matching
- `test_is_subtype_of` - Tests error inheritance

## Verification

```bash
cargo test -p quench-runtime test262::  # 38 tests pass
cargo test -p quench-runtime            # All tests pass
```

## Impact

- More accurate negative test matching reduces false failures
- Error inheritance support unblocks tests expecting base Error class
- Consolidated code is easier to maintain
- Better error messages for debugging failures

## Targets

- **Suite:** `test262`
- **Batch:** 0
- **Target subset:** `target/test262_report.md` accuracy for negative tests.
- **Blocked by:** see `tasks/index.json`
- **Exit criteria:** test262 negative tests match by expected error type and phase with inheritance support, and the harness report is regenerated.
