# Task 070: Implement overflow and aspectRatio Props

## Status: PENDING

## Goal
Implement `overflow`, `overflowX`, `overflowY`, and `aspectRatio` props for Box components to achieve 100% Ink API parity.

## Why This Matters
These props control content clipping and aspect ratio constraints:
- `overflow: "hidden"` clips content that exceeds box dimensions
- `aspectRatio` maintains proportional sizing (e.g., 16:9 boxes)

## Ink API Reference

```typescript
// Ink 7.0.5 Box props
overflow?: "visible" | "hidden"
overflowX?: "visible" | "hidden"
overflowY?: "visible" | "hidden"
aspectRatio?: number
```

## Implementation Plan

### overflow / overflowX / overflowY

Yoga layout engine handles `overflow: hidden` natively:

```rust
// In src/ink/node.rs
if let Some(PropValue::String(s)) = props.get("overflow") {
    node.yoga.set_overflow(match s.as_str() {
        "hidden" => yoga::Overflow::Hidden,
        "visible" => yoga::Overflow::Visible,
        _ => yoga::Overflow::Visible,
    });
}

// overflowX and overflowY aliases
if let Some(PropValue::String(s)) = props.get("overflowX") {
    // Yoga doesn't have separate X/Y overflow, use overflow as fallback
    node.yoga.set_overflow(match s.as_str() {
        "hidden" => yoga::Overflow::Hidden,
        _ => yoga::Overflow::Visible,
    });
}
```

**Note:** Yoga only has a single `overflow` property, not separate X/Y. `overflowX`/`overflowY` will set the same underlying property (partial support documented).

### aspectRatio

Yoga 3.0+ supports `aspect_ratio` natively:

```rust
// In src/ink/node.rs
if let Some(PropValue::Number(n)) = props.get("aspectRatio") {
    node.yoga.set_aspect_ratio(OrderedFloat(*n as f32));
}
```

**Note:** Requires yoga crate with aspect_ratio support. If not available, document as unsupported.

## Acceptance Criteria

- [ ] `overflow="hidden"` clips child content exceeding box bounds
- [ ] `overflow="visible"` allows content to overflow (default)
- [ ] `overflowX`/`overflowY` set the overflow property (documented as partial)
- [ ] `aspectRatio={16/9}` constrains width/height proportionally
- [ ] `aspectRatio={1}` creates square boxes
- [ ] Props validated in compat.rs
- [ ] Visual parity verified against Deno/Ink

## Files to Modify

- `src/ink/node.rs` — Add overflow and aspectRatio handling
- `src/compat.rs` — Move from UNSUPPORTED_BOX_PROPS to SUPPORTED_BOX_PROPS

## Example for Testing

```tsx
<Box width={20} height={10} overflow="hidden" borderStyle="round">
  <Text>This is very long text that should be clipped at the box boundary</Text>
</Box>
```

## References
- Yoga Overflow: https://yogalayout.com/docs/overflow
- Yoga Aspect Ratio: https://yogalayout.com/docs/aspect-ratio
