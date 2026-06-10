# Task 056: Render Parity — Missing Props

## Status
✅ **Complete** — All critical and most optional props implemented.

## Implemented (✅)

### Box Props
| Prop | Status |
|------|--------|
| `backgroundColor` | ✅ Fills box background + borders with color |
| `padding` | ✅ Yoga layout + ratatui Block padding |
| `paddingY` / `paddingX` | ✅ Block padding symmetric |
| `borderStyle` | ✅ Mapped to ratatui BorderType (Plain, Rounded, Thick, Double) |
| `borderColor` | ✅ Sets ratatui border foreground color |
| `borderDimColor` | ✅ Adds `Modifier::DIM` to border style |
| `borderTop` / `borderBottom` / `borderLeft` / `borderRight` | ✅ Boolean props enable specific sides |
| `marginY` / `marginX` | ✅ Yoga top+bottom / left+right margin |

### Text Props
| Prop | Status |
|------|--------|
| `color` | ✅ Named + hex colors (`#rrggbb`, `#rgb`) |
| `backgroundColor` | ✅ Named + hex colors |
| `bold` | ✅ Modifier::BOLD |
| `dimColor` | ✅ Modifier::DIM |
| `italic` | ✅ Modifier::ITALIC |
| `strikethrough` | ✅ Modifier::CROSSED_OUT |
| `underline` | ✅ Modifier::UNDERLINED |
| `inverse` | ✅ Modifier::REVERSED |
| `transform` | ✅ uppercase / lowercase |

### Yoga / Layout Props
| Prop | Status |
|------|--------|
| `margin` / `marginTop` / `marginBottom` / `marginLeft` / `marginRight` | ✅ |
| `padding` / `paddingTop` / `paddingBottom` / `paddingLeft` / `paddingRight` | ✅ |
| `flexDirection` / `alignItems` / `justifyContent` / `flexWrap` / `display` | ✅ |
| `width` / `height` (number, any %, auto) | ✅ Fixed: was hardcoded to 100% only |
| `minWidth` / `maxWidth` / `minHeight` / `maxHeight` (number, %) | ✅ |
| `flexBasis` (number, %, auto) | ✅ |
| `flexGrow` / `flexShrink` from props | ✅ (Spacer hardcodes 1/1; props override for other nodes) |
| `position` (absolute) | ✅ |

### Layout Accuracy Fixes
| Fix | Status |
|-----|--------|
| `calculate_layout` uses terminal size | ✅ Fixed: was hardcoded to 512×512 |
| Float→cell rounding | ✅ `round()` for positions, `ceil()` for dimensions |
| Hex color parsing | ✅ `#rrggbb` and `#rgb` formats |

## Known Limitations (Out of Scope)

| Feature | Note |
|---------|------|
| `borderTopColor` etc. | ❌ Not implemented — ratatui `Block` has single `border_style`, not per-side |
| `wrap` (Text) | ❌ Not implemented — Ink supports `wrap="truncate"` etc. |
| `overflow` (Box) | ❌ Not implemented — `overflow="hidden"` |
| `gap` / `rowGap` / `columnGap` | ❌ Not implemented — Yoga v0.5 lacks gap support |

## Acceptance Criteria
- [x] `backgroundColor` on Box fills background with color (including borders)
- [x] `backgroundColor` on Text sets text background
- [x] `borderColor` sets border foreground color
- [x] `borderDimColor` applies dim modifier to borders
- [x] `borderTop` / `borderBottom` / `borderLeft` / `borderRight` boolean props work
- [x] `marginY` sets Yoga top+bottom margin
- [x] `marginX` sets Yoga left+right margin
- [x] `padding` creates visual padding inside ratatui Block
- [x] `borderStyle` maps to correct ratatui BorderType
- [x] `width` / `height` accept any percentage (e.g. `"50%"`)
- [x] `minWidth`, `maxWidth`, `minHeight`, `maxHeight` from props work
- [x] `flexGrow`, `flexShrink`, `flexBasis` from props work
- [x] `transform` (uppercase/lowercase) works
- [x] Hex colors (`#rrggbb`, `#rgb`) parse correctly
- [x] Yoga layout uses actual terminal dimensions
- [x] Layout float→cell conversion uses proper rounding

## Dependencies
- Task 025 (Box render)
- Task 026 (Text render)

## SPEC Reference
§3 Rust Modules (render.rs)
