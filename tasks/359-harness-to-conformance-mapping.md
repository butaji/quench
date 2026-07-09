# Task 359: Map Harness Files to Conformance Areas

## Status: BACKLOG

## Goal

Create a mapping from test262 harness files to conformance areas and estimate conformance % increase when each is implemented. This guides prioritization of harness implementation work.

## Harness → Conformance Area Mapping

| Harness File | Conformance Area | Files Blocked | Priority |
|--------------|------------------|---------------|----------|
| `assert.js` | ALL (basic assertions) | ~50,000 | ✅ Already supported |
| `sta.js` | ALL (Test262Error) | ~50,000 | ✅ Already supported |
| `eq.js` | Basic equality | ? | ✅ Already supported |
| `propertyHelper.js` | Array, Object, built-ins | ~10,000 | 🔴 High |
| `nativeErrors.js` | Error, NativeErrors | ~200 | 🔴 High |
| `deepEqual.js` | Various | ~5,000 | 🟡 Medium |
| `compareIterator.js` | Iterator, AsyncIterator | ~1,000 | 🟡 Medium |
| `asyncHelpers.js` | Promise, async/await | ~800 | 🟡 Medium |
| `promiseHelper.js` | Promise | ~800 | 🟡 Medium |
| `typeCoercion.js` | Type coercion | ~500 | 🟢 Low |
| `regExpUtils.js` | RegExp | ~2,000 | 🟢 Low |
| `testTypedArray.js` | TypedArray | ~1,500 | 🟢 Low |
| `atomicsHelper.js` | Atomics | ~400 | 🟢 Low |

## Estimated Conformance Impact

Based on test262 file counts:

| Phase | Files Added | Est. Tests Unblocked | Cumulative % |
|-------|-------------|---------------------|--------------|
| Current | 3 | ~1,000 (basic) | ~2% |
| Phase A (propertyHelper) | +1 | +10,000 | ~20% |
| Phase A (nativeErrors) | +1 | +200 | ~20.5% |
| Phase A (deepEqual) | +1 | +5,000 | ~30% |
| Phase B (async/promise) | +2 | +1,600 | ~33% |
| Phase C (others) | +6 | ~5,000 | ~45% |

## Implementation Sequence

### Step 1: Count current skip reasons

```bash
# Run test262 and capture skip reasons
cargo test --test test262 -- --ignored 2>&1 | \
  grep "unsupported include" | \
  sort | uniq -c | sort -rn
```

This gives exact counts of which harness files are blocking tests.

### Step 2: Implement in priority order

Based on blocking count, not just file count.

### Step 3: Track progress

After each harness implementation, run subset and record:
- Tests now passing: X
- Tests still skipped: Y
- New failures (due to missing features): Z

## Verification

```bash
# Before and after comparison
echo "=== Before ==="
cargo test --test test262 -- --ignored 2>&1 | grep -E "passed|failed|skipped" | tail -5

# After implementation
echo "=== After ==="
cargo test --test test262 -- --ignored 2>&1 | grep -E "passed|failed|skipped" | tail -5
```

## Tasks Reference

- **Task 357**: Implement assert.compareArray and assert.arrayContains
- **Task 358**: Expand SUPPORTED_INCLUDES
- **Task 251**: Implement Promise (unblocks async/promise helpers)

## Exit Criteria

- [ ] All harness files analyzed and mapped
- [ ] Implementation sequence documented
- [ ] Progress tracked with before/after metrics
