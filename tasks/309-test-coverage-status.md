# Task 309: Test Coverage Status

## Status: IN PROGRESS

## Summary

**187 tests pass** in the core runtime. New tests for unimplemented features are failing as expected.

## Stable Tests (Passing)

| Test File | Passed | Ignored | Status |
|-----------|--------|---------|--------|
| lib.rs unit tests | 57 | 0 | ✓ Stable |
| conformance.rs | 2 | 2 | ✓ Stable |
| depth_limit.rs | 9 | 0 | ✓ Stable |
| equality_operators.rs | 14 | 0 | ✓ Stable |
| modules.rs | 5 | 0 | ✓ Stable |
| native_extensions.rs | 8 | 0 | ✓ Stable |
| project.rs | 6 | 1 | ✓ Stable |
| rest_parameters.rs | 8 | 0 | ✓ Stable |
| runtime_issues_basic.rs | 46 | 0 | ✓ Stable |
| runtime_issues_math.rs | 11 | 0 | ✓ Stable |
| runtime_issues_number.rs | 11 | 0 | ✓ Stable |
| test262.rs | 0 | 4 | ✓ Stable |
| to_primitive.rs | 10 | 0 | ✓ Stable |
| **Total** | **187** | | |

## New Tests (Failing - Expected)

These tests were added by sub-agents to verify unimplemented features:

| Test File | Passed | Failed | Missing Feature |
|-----------|--------|--------|----------------|
| runtime_issues_constructors.rs | 4 | 4 | Array/Error constructors |
| runtime_issues_errors.rs | ? | ? | Error handling |
| runtime_issues_expressions.rs | ? | ? | Optional chaining, template literals |
| runtime_issues_scope.rs | ? | ? | Variable scoping |
| scenarios.rs | 12 | 3 | Optional chaining, template literals, array push/pop |
| var_hoisting_tdz.rs | ? | ? | Var hoisting / let/const TDZ |

## Fixes Applied

1. **Thread-safety in depth tracking** (Task 308)
   - Fixed flaky test failures caused by shared global depth counter
   - Changed to thread-local storage for per-thread depth tracking
   - All 46 runtime_issues_basic tests now pass consistently

2. **typeof undeclared returns "undefined"** (Task 291)
   - Implemented soft lookup for typeof operand
   - Returns "undefined" for undeclared identifiers per ECMA-262

3. **Removed Context::globals duplicate** (Task 286)
   - Simplified Context struct to use only Environment
   - Removed redundant HashMap storage

## Next Steps

The following features need implementation to pass the failing tests:
- Task 289: Register Array, Error, Date as constructors
- Task 290: Complete expression syntax gaps (optional chaining, template literals)
- Task 292: Implement var hoisting and let/const TDZ
- Task 283: String.prototype sharing optimization
