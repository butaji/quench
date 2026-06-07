# Task: Re-enable Disabled Spec Test Modules

**Priority:** P1-High  
**Phase:** 3 — Coverage Gaps  
**Status:** ✅ COMPLETED
**Depends on:** 020, 021

## Problem

Previously, **11 of 15** test modules in `src/transpile/tests/mod.rs` were commented out, leaving almost zero automated coverage for control flow, data structures, variables/functions, JSX, modules, classes, stdlib, and roundtrip behavior.

## Solution

Uncommented all 15 test modules. For tests that failed due to unimplemented features, added `#[ignore = "reason"]` attributes with documented reasons instead of disabling entire modules.

## Test Results

```
test result: ok. 864 passed; 0 failed; 99 ignored; 0 measured; 0 filtered out
```

## Ignored Tests Breakdown

| Module | Ignored | Reason |
|--------|---------|--------|
| `spec_async_runtime` | 5 | Async runtime patterns not in compile path scope |
| `spec_classes` | 3 | Class support not in compile path scope |
| `spec_data_structures` | 6 | Advanced destructuring (rest, defaults, nested) not implemented |
| `spec_vars_functions` | 7 | Arrow function params, multi-declarators not implemented |
| `spec_roundtrip` | 5 | Interface/type parsing for HIR roundtrip not fully implemented |
| `completeness_codegen` | 1 | Spread expression edge case |
| `parser` | 6 | JSX text coalescing, HIR JSON serialization |
| (others) | 66 | Various intentional skips |

## Acceptance Criteria

- [x] All 15 modules uncommented and compiling.
- [x] `cargo test --bin runts` exits 0.
- [x] Zero `// #[cfg(test)]` lines remain in `src/transpile/tests/mod.rs`.
- [x] Every ignored test has a reason comment.
