# Task 090: Fix Timer ID Counter Never Resets

## Status: 🟡 **MODERATE — NOT STARTED**

## Goal
Reset `next_id` counters on `destroy_root` or document why they don't need to.

## Problem

`InkRuntime.next_id` (for nodes) and `NEXT_TIMER_ID` (for timers) increment forever:

```rust
// ink/runtime.rs
pub struct InkRuntime {
    pub(crate) next_id: u32,
    // ...
}

// bridge/timers.rs
static NEXT_TIMER_ID: AtomicU32 = AtomicU32::new(1);
```

`destroy_root()` clears the nodes Vec but does NOT reset `next_id`. On app restart or hot reload, IDs keep growing. With `u32` max at 4 billion, this is practically fine for normal use but indicates incomplete cleanup.

## Impact

- `destroy_root()` followed by `create_root()` starts IDs from wherever they left off
- After ~4 billion nodes over the lifetime of the process, IDs wrap to 0 (collision)
- Inconsistent state: root is destroyed but counter state persists

## Fix

```rust
// ink/runtime.rs
pub fn destroy_root(&mut self, root_id: u32) {
    if self.root_id == Some(root_id) {
        self.nodes.clear();
        self.root_id = None;
        self.next_id = 1;  // Reset counter
    }
}
```

For timers, add `__ink_reset_all_timers()` that clears both the HashMap and the atomic counter.

## Acceptance Criteria
- [ ] `destroy_root()` resets `next_id` to 1
- [ ] Timer counter resets when all timers cleared
- [ ] Test: create nodes, destroy root, create new root — IDs start from 1
- [ ] `cargo test` passes

## Files to Modify
- `src/ink/runtime.rs` — Reset next_id in destroy_root
- `src/bridge/timers.rs` — Reset counter in clear_all

## References
- Task 002 (Bridge: Root Node Lifecycle)
