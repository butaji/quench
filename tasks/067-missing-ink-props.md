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
| `alignContent` | string | Multi-line alignment for wrapped content | ✅ |
| `top` | number | Position from top (when position=absolute) | ✅ |
| `right` | number | Position from right (when position=absolute) | ✅ |
| `bottom` | number | Position from bottom (when position=absolute) | ✅ |
| `left` | number | Position from left (when position=absolute) | ✅ |

### Text Props (HIGH priority)

| Prop | Type | Description | Status |
|------|------|-------------|--------|
| `wrap` | string | Text wrapping (Ink 7 uses `wrap` instead of `textWrap`) | ✅ |

## Core Layout Props Supported ✅

| Prop | Type | Description | Status |
|------|------|-------------|--------|
| `flexDirection` | string | row, column, row-reverse, column-reverse | ✅ |
| `alignItems` | string | flex-start, center, flex-end, stretch, baseline | ✅ |
| `alignSelf` | string | Override parent's alignItems | ✅ |
| `alignContent` | string | Multi-line alignment (wrap) | ✅ |
| `justifyContent` | string | flex-start, center, flex-end, space-* | ✅ |
| `flexWrap` | string | nowrap, wrap, wrap-reverse | ✅ |
| `flexGrow` | number | Grow factor | ✅ |
| `flexShrink` | number | Shrink factor | ✅ |
| `flexBasis` | number/string | Initial size | ✅ |
| `gap` | number | Both row and column gap | ✅ |
| `gapX` | number | Column gap (Ink 6) | ✅ |
| `gapY` | number | Row gap (Ink 6) | ✅ |
| `columnGap` | number | Column gap (Ink 7 alias) | ✅ |
| `rowGap` | number | Row gap (Ink 7 alias) | ✅ |

## Remaining Props

### Box Props (MEDIUM priority - border colors)

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
| `aspectRatio` | number | Aspect ratio constraint | ❌ |
| `overflow` | string | Overflow handling | ❌ |
| `overflowX` | string | Horizontal overflow | ❌ |
| `overflowY` | string | Vertical overflow | ❌ |
| `borderBackgroundColor` | string | Border background color | ❌ |
| `border*BackgroundColor` | string | Individual border backgrounds | ❌ |

### Hooks Implemented (with partial support)

| Hook | Description | Status | Notes |
|------|-------------|--------|-------|
| `useAnimation` | Built-in animation helper (frame, time, delta, reset) | ✅ | Shared timer, accurate timing |
| `useWindowSize` | Terminal dimensions | ✅ | Poll-based (500ms) |
| `useCursor` | Cursor positioning | ✅ | Position tracking only |
| `usePaste` | Paste event handling | ✅ | Handler registered |
| `useBoxMetrics` | Measure box dimensions | ✅ | Poll-based (500ms) |
| `useIsScreenReaderEnabled` | Screen reader detection | ✅ | Returns `false` — no screen reader API in terminal |

### Accessibility Props (Accepted, no-op in terminal)

Ink passes `aria-*` props to the React DOM for screen reader support. Quench accepts these props silently (no warnings) since they're valid Ink API, but they have no observable effect in a terminal environment. This ensures Ink apps using accessibility props run without modification.

| Prop | Status | Notes |
|------|--------|-------|
| `aria-label` | ✅ accepted | Silently ignored (no screen reader API in terminal) |
| `aria-hidden` | ✅ accepted | Silently ignored |
| `aria-role` | ✅ accepted | Silently ignored |
| `aria-state` | ✅ accepted | Silently ignored |

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

### 3. alignContent (Done)
In `src/ink/node.rs`:
```rust
if let Some(PropValue::String(s)) = props.get("alignContent") {
    node.yoga.set_align_content(match s.as_str() {
        "center" => Align::Center,
        "flex-end" => Align::FlexEnd,
        "flex-start" => Align::FlexStart,
        "stretch" => Align::Stretch,
        "space-between" => Align::SpaceBetween,
        "space-around" => Align::SpaceAround,
        _ => Align::FlexStart,
    });
}
```

### 4. Position props (Done)
In `src/ink/node.rs`:
```rust
fn apply_position_props(node: &mut InkNode, props: &HashMap<String, PropValue>) {
    if let Some(v) = props.get("top").and_then(parse_pos) {
        node.yoga.set_position(yoga::Edge::Top, StyleUnit::Point(OrderedFloat(v)));
    }
    // ... same for right, bottom, left
}
```

### 5. wrap alias (Done)
In `src/render.rs`, added support for both `wrap` and `textWrap` props with Ink 7 modes.

## Verification
1. Check `scripts/parity.sh` for examples using new props
2. Visual verification in tmux for positioning and alignment
3. Run `examples/align-demo.tsx` for alignSelf/alignContent demo
4. Run `examples/use-animation.tsx` for useAnimation demo

## Implementation Details

### 6. useAnimation (Done)
In `src/runtime.js`, shared timer implementation:
```javascript
// All useAnimation hooks share one timer
let animationTimerId = null;
let animationHooks = [];

function useAnimation(options) {
  const { frame, time, delta, reset } = useAnimation({ interval, isActive });
  // Returns: { frame, time, delta, reset }
}
```

### 7. useWindowSize (Done)
In `src/runtime.js`, poll-based terminal size:
```javascript
function useWindowSize() {
  const [size, setSize] = useState(() => {
    const ts = __ink_get_terminal_size();
    return { columns: ts.width, rows: ts.height };
  });
  // Polls every 500ms for changes
}
```

### 8. Transform Component (Done)
In `src/runtime.js`, basic implementation:
```javascript
function Transform({ children, transform }) {
  return createElement('ink-text', { transform }, children);
}
```

## References
- Ink 7.0.5 types: https://unpkg.com/ink@7.0.5/build/index.d.ts
- Yoga layout: https://yogalayout.com/docs/
