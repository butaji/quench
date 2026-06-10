# Task 089: Fix Children Vec Cloned on Every Render

## Status: 🟠 **SIGNIFICANT PERFORMANCE — NOT STARTED**

## Goal
Eliminate `Vec<u32>` cloning in `__ink_get_node_children` to reduce per-frame allocations.

## Problem

`src/bridge/tree.rs::__ink_get_node_children()` clones the entire children vector on every call:

```rust
pub fn __ink_get_node_children(node_id: u32) -> Option<Vec<u32>> {
    INK_RUNTIME.with(|runtime| {
        runtime.borrow().node(node_id).map(|n| n.children.clone())
    })
}
```

Every frame, for every node, the entire children vector is cloned. A 50-node tree with 5 children each = **250 Vec allocations per frame** at 60fps = **15,000 allocations/second**.

## Fix Approach

Restructure `render_node` to take `&InkNode` directly (see Task 075) and iterate children by reference:

```rust
fn render_node(node: &InkNode, buf: &mut Buffer, area: Rect) {
    // ... render self ...
    for &child_id in &node.children {  // No clone! Iterates by reference
        if let Some(child) = get_node(child_id) {
            render_node(child, buf, area);
        }
    }
}
```

This requires holding a borrow of the runtime (or the node) for the duration of rendering, which conflicts with the current FFI-based renderer. Task 075 (eliminate FFI from render path) is a prerequisite or should be done together.

## Acceptance Criteria
- [ ] `render_node` iterates children without cloning
- [ ] `__ink_get_node_children` no longer called from render path
- [ ] Frame budget improved in benchmarks
- [ ] All rendering tests pass

## Files to Modify
- `src/render.rs` — Pass `&InkNode` instead of `node_id: u32`
- `src/bridge/tree.rs` — `__ink_get_node_children` can stay for other callers

## References
- Task 075 (Eliminate FFI from Render Path)
