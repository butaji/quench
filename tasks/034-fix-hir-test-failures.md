# Task: Fix or Document Stale HIR Test Failures

**Priority:** P1-High  
**Phase:** 2 — Compile + Verification  
**Depends on:** 033  
**Status:** COMPLETED

## Summary

Fixed 33 failing tests by adding `#[ignore]` attributes with reason comments. All tests now pass with `cargo test --bin runts`.

## Root Cause Analysis

The failures were categorized into three types:

1. **Features not in compile path scope** (most common):
   - Async runtime patterns (`fetch`, `Promise`, `for await`)
   - Class support (`class_with_constructor_codegen`, etc.)
   - Advanced destructuring (nested, with defaults, rest patterns)
   - Arrow function parameter patterns
   - Export default function handling
   - Multiple variable declarators

2. **Parser/converter not implemented**:
   - Some features intentionally skipped in the parser converter

3. **Roundtrip tests**:
   - Interface/type parsing not fully implemented for HIR roundtrip

## Files Modified

| File | Changes |
|------|---------|
| `src/transpile/tests/spec_async_runtime.rs` | Added `#[ignore]` to 5 failing tests |
| `src/transpile/tests/spec_classes.rs` | Added `#[ignore]` to 3 failing tests + inner submodules |
| `src/transpile/tests/spec_data_structures/destructuring_array.rs` | Added `#[ignore]` to 1 failing test |
| `src/transpile/tests/spec_data_structures/destructuring_object.rs` | Added `#[ignore]` to 3 failing tests |
| `src/transpile/tests/spec_data_structures/pattern_coverage.rs` | Added `#[ignore]` to 2 failing tests |
| `src/transpile/tests/spec_vars_functions/array_destructuring.rs` | Added `#[ignore]` to 1 failing test |
| `src/transpile/tests/spec_vars_functions/arrow_functions.rs` | Added `#[ignore]` to 1 failing test |
| `src/transpile/tests/spec_vars_functions/function_parameters.rs` | Added `#[ignore]` to 3 failing tests |
| `src/transpile/tests/spec_vars_functions/object_destructuring.rs` | Added `#[ignore]` to 2 failing tests |
| `src/transpile/tests/spec_roundtrip.rs` | Added `#[ignore]` to 5 failing tests |
| `src/transpile/tests/completeness_codegen.rs` | Added `#[ignore]` to 1 failing test |
| `src/transpile/tests/parser.rs` | Added `#[ignore]` to 6 failing tests |

## Test Results

```
test result: ok. 864 passed; 0 failed; 99 ignored; 0 measured; 0 filtered out
```

## Acceptance Criteria

- [x] Task 033 completed (all 15 modules enabled).
- [x] `cargo test --bin runts` exits 0.
- [x] No panics on `Expr::Invalid` in quote_codegen.
- [x] Every `#[ignore]`d test has a reason comment.
- [x] Intentionally skipped features documented: async runtime, class support, advanced destructuring, multi-declarators, roundtrip interfaces — all marked with `#[ignore = "reason"]` in test files.

## Notes

- These failures are **compile-path only** — the dev path (TSX→JS→rquickjs) bypasses HIR entirely.
- All ignored tests have descriptive reason comments explaining why they are skipped.
- The compile path is primarily used for static analysis; the dev path using rquickjs is the primary execution engine.
