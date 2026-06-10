# Task 002: Bridge: Root Node Lifecycle

## Status
✅ **Done**


## Goal
Implement `__ink_create_root` and `__ink_destroy_root` to manage the terminal root Yoga node.

## Acceptance Criteria
- [ ] `__ink_create_root()` → `u32` creates a Yoga node with `width: 100%`, `height: 100%` of terminal.
- [ ] `__ink_destroy_root(id)` drops the Yoga tree and all child nodes.
- [ ] Unit test: create + destroy root, assert no leaks via node count.

> ⚠️ **Known issue:** `destroy_root()` clears the node storage but does NOT reset `next_id`, so IDs grow forever. See Task 090.

## Dependencies
- Task 001

## SPEC Reference
§3 Rust Modules (bridge/node.rs, ink/runtime.rs)
