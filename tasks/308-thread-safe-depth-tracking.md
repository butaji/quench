# Task 308: Fix thread-safety bug in depth tracking

## Status: COMPLETED

## Problem

The depth tracking in `crates/quench-runtime/src/interpreter/depth.rs` used a global `static CURRENT_DEPTH: AtomicUsize` that was shared across all test threads. When tests ran in parallel, they would interfere with each other's depth counters, causing flaky test failures.

The symptom was that tests like `test_array_flat_map`, `test_closure_basic`, and `test_arguments_object_*` would fail with "Maximum call stack size exceeded" errors even though the actual recursion depth was low.

## Root Cause

When multiple test threads ran concurrently:
1. Thread A would increment the global counter to, say, 5
2. Thread B would call `reset_depth()`, setting counter to 0
3. Thread A would continue with incorrect assumptions about its depth
4. The counter would get corrupted

## Fix

Changed `CURRENT_DEPTH` from a global atomic to a `thread_local!` storage. Each thread now has its own depth counter:

```rust
thread_local! {
    static CURRENT_DEPTH: Cell<usize> = const { Cell::new(0) };
}
```

The `MAX_RECURSION_DEPTH` remains a global atomic because tests need to set it from the main thread and have it take effect in spawned threads.

## Files Modified

- `crates/quench-runtime/src/interpreter/depth.rs`

## Acceptance Criteria

- [x] Tests run in parallel without flaky failures
- [x] All 46 `runtime_issues_basic` tests pass consistently
- [x] All 9 `depth_limit` tests pass
- [x] `test_depth_reset_after_context_creation` passes (spawns threads with different depth limits)

## Verification

```bash
cargo test -p quench-runtime --test runtime_issues_basic  # 46 tests pass
cargo test -p quench-runtime --test depth_limit  # 9 tests pass
```

## Targets

- **Suite:** `runtime`
- **Batch:** 6
- **Target subset:** n/a (interpreter infrastructure)
- **Blocked by:** none
- **Exit criteria:** Depth counter is thread-local; parallel `runtime_issues` tests pass consistently.
