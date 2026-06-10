# Task 075: Eliminate FFI Round-Trips from Render Path

## Status: 🔴 **CRITICAL PERFORMANCE — NOT STARTED**

## Goal
Remove all `__ink_call` / bridge function invocations from `src/render.rs` and pass node data directly.

## Problem

`render_node()` and its helpers call bridge functions for **every property lookup on every node on every frame**:

```rust
// render.rs — EVERY FRAME, for EVERY node:
let tag = bridge::__ink_get_node_tag(node_id);           // FFI lock + lookup
let layout = bridge::__ink_get_layout(node_id);          // FFI lock + lookup
let border_color = bridge::__ink_get_node_prop(node_id, "borderColor")
    .map(|s| s.trim_matches('"').to_string());           // FFI lock + lookup + JSON stringify + trim
```

For a 50-node tree with ~5 props each, this is **250+ FFI calls per frame**, each:
1. Locking the thread-local `INK_RUNTIME` (`RefCell`)
2. Looking up the node in `Vec<Option<InkNode>>`
3. Serializing the prop value to JSON string
4. Returning across the FFI boundary
5. Deserializing/trimming the string on the render side

All of this data lives in the **same process**. The renderer should just hold a reference.

## Fix Approach

### Phase 1: Add direct node accessors (no FFI)

Add a function to `src/ink/runtime.rs` (or `src/ink/shared.rs`) that returns a shared reference to a node:

```rust
// In src/ink/shared.rs or new src/ink/query.rs
pub fn with_node<F, R>(node_id: u32, f: F) -> Option<R>
where
    F: FnOnce(&InkNode) -> R,
{
    INK_RUNTIME.with(|rt| {
        rt.borrow().node(node_id).map(f)
    })
}
```

### Phase 2: Rewrite render.rs to use direct references

Change `render_node` signature:

```rust
// BEFORE:
fn render_node(node_id: u32, buf: &mut Buffer, area: Rect)

// AFTER:
fn render_node(node: &InkNode, buf: &mut Buffer, area: Rect)
```

The top-level `render_tree()` can use `with_node` or `with_runtime` to get the root, then recurse with `&InkNode`:

```rust
fn render_tree(...) -> Result<()> {
    terminal.draw(|frame| {
        INK_RUNTIME.with(|rt| {
            let rt = rt.borrow();
            if let Some(root) = rt.root_id().and_then(|id| rt.node(id)) {
                render_node(root, frame.buffer_mut(), frame.area());
            }
        });
    })?;
    Ok(())
}
```

### Phase 3: Remove JSON stringification from render queries

Replace `__ink_get_node_prop` (returns JSON string) with direct `PropValue` access:

```rust
// BEFORE:
let border_color = bridge::__ink_get_node_prop(node_id, "borderColor")
    .map(|s| s.trim_matches('"').to_string())
    .and_then(|s| parse_color(&s));

// AFTER:
let border_color = node.props.get("borderColor")
    .and_then(|v| match v {
        PropValue::String(s) => parse_color(s),
        _ => None,
    });
```

## Performance Impact

| Metric | Before | After | Improvement |
|--------|--------|-------|-------------|
| FFI calls per 50-node frame | ~250 | 0 | **Infinite** (eliminated) |
| String allocs per frame | ~250 | 0 | **Eliminated** |
| Render latency (est.) | ~2-3ms | ~0.5ms | **4-6x** |

## Acceptance Criteria
- [ ] `render.rs` contains zero `bridge::__ink_*` calls
- [ ] `render_node` takes `&InkNode` instead of `node_id: u32`
- [ ] Children are rendered by looking up `node.children` and resolving IDs to `&InkNode`
- [ ] `__ink_get_node_prop_raw` is used instead of `__ink_get_node_prop` (no JSON stringification)
- [ ] All rendering tests pass (visual parity preserved)
- [ ] Frame budget improved in benchmarks

## Files to Modify
- `src/render.rs` — Rewrite to use `&InkNode` directly
- `src/ink/shared.rs` — Add `with_node` / `with_runtime` helpers
- `src/ink/runtime.rs` — Ensure `node()` returns `Option<&InkNode>` (already does)

## References
- Task 025-029 (Rendering tasks — original implementation)
- Task 055 (Hot Path Optimization)
