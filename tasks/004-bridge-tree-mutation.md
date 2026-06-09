# Task 004: Bridge: Tree Mutation

## Goal
Implement tree mutation operations: append, remove, insert before.

## Acceptance Criteria
- [ ] `__ink_append_child(parent, child)` adds child to parent’s Yoga node and child list.
- [ ] `__ink_remove_child(parent, child)` detaches child, marks dirty.
- [ ] `__ink_insert_before(parent, child, before)` reorders child.
- [ ] Unit test: build 3-level tree, remove middle node, verify remaining structure.

## Dependencies
- Task 003

## SPEC Reference
§4 Bridge API — append_child / remove_child / insert_before
