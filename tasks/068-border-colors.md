# Task 068: Implement Individual Border Colors

## Status: PENDING

## Goal
Implement per-side border colors (`borderTopColor`, `borderBottomColor`, `borderLeftColor`, `borderRightColor`) and their dim/background variants for 100% Ink API parity.

## Why This Matters
Ink apps use individual border colors to create visual hierarchy — e.g., a red top border for errors, green bottom for success. Without this, TuiBridge silently ignores these props and all borders render with the same color.

## Ink API Reference

```typescript
// Ink 7.0.5 Box props
borderTopColor?: string
borderBottomColor?: string
borderLeftColor?: string
borderRightColor?: string
borderTopDimColor?: boolean
borderBottomDimColor?: boolean
borderLeftDimColor?: boolean
borderRightDimColor?: boolean
borderBackgroundColor?: string
borderTopBackgroundColor?: string
borderBottomBackgroundColor?: string
borderLeftBackgroundColor?: string
borderRightBackgroundColor?: string
```

## Blocker

ratatui's `Block` widget applies a single `border_style` to all borders. There is no native per-side color support.

## Potential Approaches

### Option A: Custom Border Rendering (Recommended)
Instead of using ratatui's `Block::borders()`, manually draw each border side with its own style:

```rust
// In render_box(), after calculating rect:
if let Some(top_color) = get_border_color(node_id, "borderTopColor") {
    draw_horizontal_line(buf, x, y, w, top_color, border_style);
}
// ... repeat for bottom, left, right
```

This requires:
1. Parsing individual border color props in `src/bridge/props.rs`
2. Adding per-side color getters in `src/bridge/node.rs`
3. Replacing `Block::render()` with manual border drawing in `src/render.rs`
4. Respecting `borderTop`/`borderBottom`/`borderLeft`/`borderRight` boolean flags

### Option B: Span-based Borders
Use ratatui `Line` with styled `Span`s for each border segment. More complex but handles `borderStyle` (round, double, etc.) automatically.

## Acceptance Criteria

- [ ] `borderTopColor` sets top border color independently
- [ ] `borderBottomColor` sets bottom border color independently
- [ ] `borderLeftColor` sets left border color independently
- [ ] `borderRightColor` sets right border color independently
- [ ] `borderTopDimColor` applies DIM modifier to top border
- [ ] `border*DimColor` variants work for all sides
- [ ] `border*BackgroundColor` variants work for all sides
- [ ] Falls back to `borderColor` when individual side color not specified
- [ ] Falls back to no border when `borderStyle` not set
- [ ] Works with existing `borderTop`/`borderBottom`/`borderLeft`/`borderRight` boolean props
- [ ] Visual parity verified in tmux against Deno/Ink

## Files to Modify

- `src/render.rs` — Manual border rendering or span-based approach
- `src/bridge/node.rs` — Add per-side color prop getters
- `src/bridge/props.rs` — Parse individual border color props
- `src/compat.rs` — Remove from UNSUPPORTED_BOX_PROPS, add to PARTIAL_PROPS if needed

## Example for Testing

```tsx
<Box borderStyle="round" borderTopColor="red" borderBottomColor="green">
  <Text>Red top, green bottom</Text>
</Box>
```

## References
- ratatui Block widget: https://docs.rs/ratatui/latest/ratatui/widgets/struct.Block.html
- Ink Box source: https://github.com/vadimdemedes/ink/blob/master/src/components/Box.tsx
