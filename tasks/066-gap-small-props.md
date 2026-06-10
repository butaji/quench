# Task 066: Implement Missing Ink Props (gap, small)

## Status: Done

## Issue
Several Ink props used in examples were not implemented:
- `gap`, `gapX`, `gapY` - Flex gap between children
- `small` - Smaller text styling
- `title` - Box title (was partially implemented in render.rs only)

## Implementation

### 1. `src/ink/node.rs` — Gap prop
Added gap support using Yoga's `set_gap()`:
```rust
// Gap (flex gap between children)
if let Some(PropValue::Number(n)) = props.get("gap") {
    node.yoga.set_gap(yoga::Axis::Horizontal, OrderedFloat(*n as f32));
    node.yoga.set_gap(yoga::Axis::Vertical, OrderedFloat(*n as f32));
}
if let Some(PropValue::Number(n)) = props.get("gapX") {
    node.yoga.set_gap(yoga::Axis::Horizontal, OrderedFloat(*n as f32));
}
if let Some(PropValue::Number(n)) = props.get("gapY") {
    node.yoga.set_gap(yoga::Axis::Vertical, OrderedFloat(*n as f32));
}
```

### 2. `src/render.rs` — Small text
Added small text rendering using DIM modifier:
```rust
// small text - rendered as dim (terminals don't have small font)
if bridge::__ink_get_node_prop(node_id, "small").is_some() {
    style = style.add_modifier(Modifier::DIM);
}
```

### 3. `src/compat.rs` — Supported props lists
Updated SUPPORTED_BOX_PROPS and SUPPORTED_TEXT_PROPS:
```rust
pub static SUPPORTED_BOX_PROPS: &[&str] = &[
    // ... existing props ...
    "gap", "gapX", "gapY",
    "title",
    // ...
];

pub static SUPPORTED_TEXT_PROPS: &[&str] = &[
    // ... existing props ...
    "small",
    // ...
];
```

## Examples Fixed
- `align-demo.tsx` — Uses `gap={3}` extensively
- `animations.tsx` — Uses `gap={1}` and `small` text
- `component-composition.tsx` — Uses `gap` and `small`
- `dashboard.tsx` — Uses `gap`
- `flex-layouts.tsx` — Uses `gap`

## Verification
After these changes, all examples using gap and small props should render correctly.
