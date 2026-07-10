# Task 358: Expand SUPPORTED_INCLUDES to Cover More Harness Files

## Status: COMPLETED

## Verification
- Commit: see git log
- Test: `cargo test -p quench-runtime --lib test262::harness_tests`
- All 21 harness tests pass including new tests for:
  - harness_verify_property_passes
  - harness_verify_property_fails_wrong_value
  - harness_verify_accessor_property
  - harness_deep_equal_passes
  - harness_deep_equal_fails
  - harness_deep_equal_arrays

## Goal

Expand `SUPPORTED_INCLUDES` in the test262 runner to include more harness files, reducing the number of skipped tests and increasing conformance coverage.

## Implementation Summary

### Updated Files

1. **crates/quench-runtime/src/test262/batches.rs**
   - Added `SUPPORTED_INCLUDES` constant with 11 entries (Task 358)
   - Refactored include checking to use the constant

2. **crates/quench-runtime/src/test262/helpers.rs** (new file)
   - `verifyProperty` - property descriptor verification
   - `verifyAccessorProperty` - accessor property verification
   - `makeNativeError` - create native error instances
   - `assert_deepEqual` - deep equality comparison
   - `deep_equal_internal` - recursive deep equality
   - `deep_equal_objects` - object comparison

3. **crates/quench-runtime/src/test262/harness.rs**
   - Refactored to use helpers from `helpers.rs`
   - Registers new helpers in `inject_harness()`

4. **crates/quench-runtime/src/test262/harness_tests.rs**
   - Added 6 new tests for the new helpers

### Supported Includes

```rust
const SUPPORTED_INCLUDES: &[&str] = &[
    // Core helpers
    "assert.js",
    "sta.js",
    "eq.js",
    // Property verification helpers (Task 358)
    "propertyHelper.js",
    // Native error constructors (Task 358)
    "nativeErrors.js",
    // Deep equality (Task 358)
    "deepEqual.js",
    // Compare arrays (Task 359)
    "compareArray.js",
    // Constructor check helper (Task 359)
    "isConstructor.js",
    // Function global object helper (Task 359)
    "fnGlobalObject.js",
    // RegExp utilities (Task 360)
    "regExpUtils.js",
    // Async test helpers (Task 361)
    "asyncHelpers.js",
    // ArrayBuffer detachment (Task 362)
    "detachArrayBuffer.js",
];
```

### Registered Helpers

- `verifyProperty` - verifies object property descriptors
- `verifyAccessorProperty` - verifies accessor properties
- `nativeErrors` - array of error constructors
- `allErrorConstructors` - all error constructors
- `makeNativeError` - creates native error instances
- `assert.deepEqual` - deep equality comparison
- `buildString` - builds strings from code point ranges
- `testPropertyOfStrings` - tests regex against string sets
- `matchValidator` - validates regex match results
- `asyncTest` - async test wrapper
- `$DETACHBUFFER` - detaches ArrayBuffer

## Line Count Compliance

All files now under 500 lines:
- batches.rs: 315 lines
- harness.rs: 357 lines
- helpers.rs: 352 lines
- harness_tests.rs: 317 lines

## Impact

Expanding harness support enables:
- `built-ins/Array/*` (property tests)
- `built-ins/Error/*` (nativeErrors)
- `built-ins/Object/*` (property tests)
- RegExp tests (regExpUtils.js)
- Async tests (asyncHelpers.js)
- ArrayBuffer tests (detachArrayBuffer.js)
