# Task 358: Expand SUPPORTED_INCLUDES to Cover More Harness Files

## Status: BACKLOG

## Goal

Expand `SUPPORTED_INCLUDES` in the test262 runner to include more harness files, reducing the number of skipped tests and increasing conformance coverage.

## Current State

In `crates/quench-runtime/src/test262/batches.rs`:

```rust
const SUPPORTED_INCLUDES = ["assert.js", "sta.js", "eq.js"];
```

Any test requiring a harness file not in this list is skipped with `"unsupported include: <file>"`.

## Available Harness Files

From `tests/test262/harness/` (run `ls tests/test262/harness/` to see all):

| File | Source | Purpose | Tests Blocked |
|------|--------|---------|---------------|
| `propertyHelper.js` | `tests/test262/harness/propertyHelper.js` | `verifyProperty`, `verifyAccessorProperty` | Array, Object, built-ins |
| `nativeErrors.js` | `tests/test262/harness/nativeErrors.js` | Error constructor arrays | Error, NativeErrors |
| `asyncHelpers.js` | `tests/test262/harness/asyncHelpers.js` | Async test helpers | Promise, async tests |
| `promiseHelper.js` | `tests/test262/harness/promiseHelper.js` | Promise test utilities | Promise tests |
| `typeCoercion.js` | `tests/test262/harness/typeCoercion.js` | Type coercion helpers | Type coercion tests |
| `deepEqual.js` | `tests/test262/harness/deepEqual.js` | Deep equality comparison | Various |
| `compareIterator.js` | `tests/test262/harness/compareIterator.js` | Iterator comparison | Iterator tests |
| `regExpUtils.js` | `tests/test262/harness/regExpUtils.js` | RegExp utilities | RegExp tests |
| `testTypedArray.js` | `tests/test262/harness/testTypedArray.js` | TypedArray helpers | TypedArray tests |
| `atomicsHelper.js` | `tests/test262/harness/atomicsHelper.js` | Atomics utilities | Atomics tests |

## Implementation Strategy

### Phase A: Minimal Helpers (Quick Wins)

Some helpers are pure JavaScript that can be loaded as-is:

1. **propertyHelper.js** - Most tests need `verifyProperty`
   - Implement as native Rust: `verifyProperty(obj, name, desc, options)`
   - Uses `assert`, `assert.sameValue`, `Object.getOwnPropertyDescriptor`
   - Delegates to Rust native implementations

2. **nativeErrors.js** - Just exports arrays
   - Define in Rust: `nativeErrors = [Error, EvalError, RangeError, ...]`
   - Simple: just expose the built-in constructors

3. **deepEqual.js** - Deep equality
   - Implement using existing `strict_eq` with recursive traversal

### Phase B: Complex Helpers (Higher Effort)

4. **asyncHelpers.js** - Requires Promise implementation (see Task 251)
5. **promiseHelper.js** - Requires Promise implementation (see Task 251)

### Phase C: Advanced Helpers (Lower Priority)

6. Other specialized helpers as needed

## Implementation Steps

### 1. Update SUPPORTED_INCLUDES

```rust
// In batches.rs
const SUPPORTED_INCLUDES: [&str; 6] = [
    "assert.js",
    "sta.js", 
    "eq.js",
    "propertyHelper.js",
    "nativeErrors.js",
    "deepEqual.js",
];
```

### 2. Register Native Helpers

```rust
// In harness.rs inject_harness()
pub fn inject_harness(ctx: &mut Context) {
    // ... existing helpers ...
    
    // propertyHelper.js
    register_native(ctx, "verifyProperty", verify_property);
    register_native(ctx, "verifyAccessorProperty", verify_accessor_property);
    
    // nativeErrors.js
    ctx.set_global("nativeErrors", make_native_array(ctx, &[
        "Error", "EvalError", "RangeError", "ReferenceError", 
        "SyntaxError", "TypeError", "URIError"
    ]));
}
```

### 3. Implement verifyProperty (Native Rust)

```rust
fn verify_property(args: Vec<Value>) -> Result<Value, JsError> {
    // 1. Get obj, name, desc from args
    // 2. Get original descriptor via Object.getOwnPropertyDescriptor
    // 3. Compare desc fields against original:
    //    - value (using SameValue)
    //    - writable
    //    - enumerable
    //    - configurable
    // 4. Optionally restore original descriptor
    // 5. Return true or throw Test262Error
}
```

## Targets

- **Suite:** test262
- **Batch:** 0
- **Blocked by:** 357 (assert.compareArray)
- **Exit criteria:** At least 5 more harness files supported; conformance % increases

## Verification

```bash
# Before: Count skipped tests
cargo test --test test262 -- --ignored 2>&1 | grep "unsupported include" | wc -l

# After: Should show fewer skipped tests
cargo test --test test262 -- --ignored 2>&1 | grep "unsupported include" | wc -l
```

## Impact

Expanding harness support unblocks:
- `built-ins/Array/*` (property tests)
- `built-ins/Error/*` (nativeErrors)
- `built-ins/Object/*` (property tests)
- Many more test directories
