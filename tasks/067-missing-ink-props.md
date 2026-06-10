# Task 067: Implement Missing Ink Props for 100% Compatibility

## Status: MOSTLY DONE (2026-06-10)

## Goal
Implement remaining Ink 7.0.5 props to achieve 100% API compatibility with Ink, not a simplified subset.

## Implemented Props ✅

### Box Props (HIGH priority)

| Prop | Type | Description | Status |
|------|------|-------------|--------|
| `columnGap` | number | Horizontal gap (alias for `gapX`) | ✅ |
| `rowGap` | number | Vertical gap (alias for `gapY`) | ✅ |
| `alignSelf` | string | Override parent's alignItems for this child | ✅ |
| `top` | number | Position from top (when position=absolute) | ✅ |
| `right` | number | Position from right (when position=absolute) | ✅ |
| `bottom` | number | Position from bottom (when position=absolute) | ✅ |
| `left` | number | Position from left (when position=absolute) | ✅ |

### Text Props (HIGH priority)

| Prop | Type | Description | Status |
|------|------|-------------|--------|
| `wrap` | string | Text wrapping (Ink 7 uses `wrap` instead of `textWrap`) | ✅ |

## Remaining Props

### Box Props (MEDIUM priority)

| Prop | Type | Description | Status |
|------|------|-------------|--------|
| `borderTopColor` | string | Individual top border color | ❌ |
| `borderBottomColor` | string | Individual bottom border color | ❌ |
| `borderLeftColor` | string | Individual left border color | ❌ |
| `borderRightColor` | string | Individual right border color | ❌ |
| `borderTopDimColor` | boolean | Dim top border | ❌ |
| `borderBottomDimColor` | boolean | Dim bottom border | ❌ |
| `borderLeftDimColor` | boolean | Dim left border | ❌ |
| `borderRightDimColor` | boolean | Dim right border | ❌ |

### Box Props (LOW priority)

| Prop | Type | Description | Status |
|------|------|-------------|--------|
| `alignContent` | string | Multi-line alignment | ❌ |
| `aspectRatio` | number | Aspect ratio constraint | ❌ |
| `overflow` | string | Overflow handling | ❌ |
| `overflowX` | string | Horizontal overflow | ❌ |
| `overflowY` | string | Vertical overflow | ❌ |

### Missing Hooks (MEDIUM priority)

| Hook | Description | Status |
|------|-------------|--------|
| `useAnimation` | Built-in animation helper (frame, time, delta, reset) | ❌ |
| `useWindowSize` | Terminal dimensions (can be alias for useStdout) | ❌ |
| `useBoxMetrics` | Measure box dimensions | ❌ |

## Implementation Details

### 1. columnGap/rowGap (Done)
In `src/ink/node.rs`, added alias handlers:
```rust
// gapX and columnGap are synonyms
if let Some(PropValue::Number(n)) = props.get("gapX").or(props.get("columnGap")) {
    node.yoga.set_gap(yoga::Axis::Horizontal, OrderedFloat(*n as f32));
}
// gapY and rowGap are synonyms
if let Some(PropValue::Number(n)) = props.get("gapY").or(props.get("rowGap")) {
    node.yoga.set_gap(yoga::Axis::Vertical, OrderedFloat(*n as f32));
}
```

### 2. alignSelf (Done)
In `src/ink/node.rs`:
```rust
if let Some(PropValue::String(s)) = props.get("alignSelf") {
    node.yoga.set_align_self(match s.as_str() {
        "center" => Align::Center,
        "flex-end" => Align::FlexEnd,
        "flex-start" => Align::FlexStart,
        "stretch" => Align::Stretch,
        "baseline" => Align::Baseline,
        "auto" => Align::Auto,
        _ => Align::Auto,
    });
}
```

### 3. Position props (Done)
In `src/ink/node.rs`:
```rust
fn apply_position_props(node: &mut InkNode, props: &HashMap<String, PropValue>) {
    if let Some(v) = props.get("top").and_then(parse_pos) {
        node.yoga.set_position(yoga::Edge::Top, StyleUnit::Point(OrderedFloat(v)));
    }
    // ... same for right, bottom, left
}
```

### 4. wrap alias (Done)
In `src/render.rs`, added support for both `wrap` and `textWrap` props.

## Verification
1. Check `scripts/parity.sh` for examples using new props
2. Visual verification in tmux for positioning and alignment

## References
- Ink 7.0.5 types: https://unpkg.com/ink@7.0.5/build/index.d.ts
- Yoga layout: https://yogalayout.com/docs/
