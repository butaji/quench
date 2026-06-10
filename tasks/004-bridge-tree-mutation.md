# Task 004: Bridge: Tree Mutation

## Status
✅ **Done**


## Goal
Implement tree mutation operations: append, remove, insert before.

## Acceptance Criteria
- [ ] `__ink_append_child(parent, child)` adds child to parent’s Yoga node and child list.
- [ ] `__ink_remove_child(parent, child)` detaches child, marks dirty.
- [ ] `__ink_insert_before(parent, child, before)` reorders child.
- [ ] Unit test: build 3-level tree, remove middle node, verify remaining structure.

## Dependencies
- Task 003

> ⚠️ **Bug identified (Task 085):** The current tree mutation implementation (`append_child`, `remove_child`, `insert_before`) derives raw pointers from shared `&InkNode` borrows and uses them after taking mutable `&mut InkNode` borrows. This is undefined behavior under Rust's aliasing model. See Task 085 for the fix.

## SPEC Reference
§3 Rust Modules (bridge/tree.rs)
