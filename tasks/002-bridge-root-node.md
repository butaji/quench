# Task 002: Bridge: Root Node Lifecycle

## Goal
Implement `__ink_create_root` and `__ink_destroy_root` to manage the terminal root Yoga node.

## Acceptance Criteria
- [ ] `__ink_create_root()` → `u32` creates a Yoga node with `width: 100%`, `height: 100%` of terminal.
- [ ] `__ink_destroy_root(id)` drops the Yoga tree and all child nodes.
- [ ] Unit test: create + destroy root, assert no leaks via node count.

## Dependencies
- Task 001

## SPEC Reference
§4 Bridge API — create_root / destroy_root
