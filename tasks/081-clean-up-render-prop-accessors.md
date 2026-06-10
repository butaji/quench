# Task 081: Clean Up Render Prop Accessors

## Status: 🟠 **POLISH — NOT STARTED**

## Goal
Replace all `__ink_get_node_prop` (JSON string) calls in `render.rs` with `__ink_get_node_prop_raw` (PropValue) or direct node access.

## Problem

`render.rs` repeatedly calls `__ink_get_node_prop` which JSON-serializes the prop value, then the renderer trims quotes and re-parses:

```rust
// render.rs
let border_color = bridge::__ink_get_node_prop(node_id, "borderColor")
    .map(|s| s.trim_matches('"').to_string())
    .and_then(|s| parse_color(&s));
```

This is wasteful because:
1. The bridge serializes `PropValue::String("red")` → `"\"red\""`
2. The renderer trims quotes → `"red"`
3. Then parses color

`__ink_get_node_prop_raw` already exists and returns the actual `PropValue` without stringification:

```rust
// bridge/tree.rs
pub fn __ink_get_node_prop_raw(node_id: u32, prop: &str) -> Option<PropValue>
```

## Fix

Replace all `__ink_get_node_prop` calls in `render.rs` with `__ink_get_node_prop_raw`:

```rust
// BEFORE:
let border_color = bridge::__ink_get_node_prop(node_id, "borderColor")
    .map(|s| s.trim_matches('"').to_string())
    .and_then(|s| parse_color(&s));

// AFTER:
let border_color = bridge::__ink_get_node_prop_raw(node_id, "borderColor")
    .and_then(|v| match v {
        PropValue::String(s) => parse_color(&s),
        _ => None,
    });
```

Also remove the now-unused `__ink_get_node_prop` from the render path entirely (though keep it for other callers if needed).

## Acceptance Criteria
- [ ] `render.rs` contains zero `bridge::__ink_get_node_prop` calls
- [ ] All prop lookups use `__ink_get_node_prop_raw` or direct `&InkNode` access
- [ ] No `.trim_matches('"')` string manipulation in `render.rs`
- [ ] All rendering tests pass
- [ ] Visual parity preserved

## Files to Modify
- `src/render.rs` — Replace prop accessors

## References
- Task 075 (Eliminate FFI from Render Path — supersedes this if done first)
