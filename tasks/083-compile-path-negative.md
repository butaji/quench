# Task 083: Add Compile-Path Negative Tests (Expected Failures)

**Priority:** P2-Medium
**Phase:** 8 — Compile-Path Integration Tests
**Depends on:** 082

## Problem

There are no tests verifying that the compile path produces helpful errors for unsupported features.

## Work

1. Create `tests/compile_codegen_negative.rs`
2. Test unsupported features produce clear error messages:
   - Dynamic imports
   - `with` statement
   - `eval()`
   - Generators (if not yet supported)
   - Classes with complex inheritance
3. Verify errors are caught at `rustc` compile time, not runtime

## Acceptance Criteria

- [ ] Negative test file exists with ≥5 tests
- [ ] Each test asserts compilation fails with expected error pattern
- [ ] All tests pass
