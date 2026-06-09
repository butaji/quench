# Task 003: Bridge: Create Nodes

## Goal
Implement `__ink_create_node` and `__ink_create_text_node` for reconciler node creation.

## Acceptance Criteria
- [ ] `__ink_create_node(tag, props)` → `u32` stores tag (`ink-box`, `ink-text`, etc.) and props in `InkNode`.
- [ ] `__ink_create_text_node(text)` → `u32` creates leaf node with text content.
- [ ] Both create corresponding `YogaNode` in Rust.
- [ ] Unit test: create nodes, verify Yoga node exists, verify tag/props stored.

## Dependencies
- Task 002

## SPEC Reference
§4 Bridge API — create_node / create_text_node
