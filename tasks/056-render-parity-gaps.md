# Task 056: Render Parity — Missing Props

## Goal
Achieve 100% look&feel parity with Ink by implementing all props used in examples.

## Gaps Found

### Box Props (Missing from main.rs)

| Prop | Used By | Status | Ink Behavior |
|------|---------|--------|--------------|
| `backgroundColor` | file-tree.js, tabs.js | ❌ Missing | Fill box background with color |
| `borderColor` | — | ❌ Missing | Color for all borders |
| `borderDimColor` | — | ❌ Missing | Dim color for borders |
| `borderTop` | — | ❌ Missing | Only top border |
| `borderBottom` | — | ❌ Missing | Only bottom border |
| `borderLeft` | — | ❌ Missing | Only left border |
| `borderRight` | — | ❌ Missing | Only right border |

### Text Props (Missing from main.rs)

| Prop | Used By | Status | Ink Behavior |
|------|---------|--------|--------------|
| `backgroundColor` | file-tree.js, tabs.js | ❌ Missing | Text background color |
| `underline` | — | ❌ Missing | Underline modifier |
| `inverse` | — | ❌ Missing | Reverse fg/bg |
| `transform` | — | ❌ Missing | uppercase/lowercase |

### Yoga Props (Missing from ink.rs `apply_props`)

| Prop | Used By | Status | Ink Behavior |
|------|---------|--------|--------------|
| `marginY` | focus-form.js, dashboard.js | ❌ Missing | margin-top + margin-bottom |
| `marginX` | — | ❌ Missing | margin-left + margin-right |
| `minWidth` | — | ❌ Missing | Minimum width constraint |
| `maxWidth` | — | ❌ Missing | Maximum width constraint |
| `minHeight` | — | ❌ Missing | Minimum height constraint |
| `maxHeight` | — | ❌ Missing | Maximum height constraint |
| `flexBasis` | — | ❌ Missing | Initial flex size before grow/shrink |
| `flexGrow` | — | ❌ Missing | Grow factor (only works for Spacer tag) |
| `flexShrink` | — | ❌ Missing | Shrink factor (only works for Spacer tag) |

### Visual Padding (Missing from main.rs)

`padding` sets Yoga layout padding but ratatui `Block` widget doesn't get `.padding()` called. Visual padding inside borders is missing.

## Critical Path

**Must have for 100% parity:**
1. `backgroundColor` (Box + Text) — used by file-tree.js, tabs.js
2. `marginY` / `marginX` — used by focus-form.js, dashboard.js
3. `padding` visual — affects nearly every example

**Should have:**
4. `borderColor` / `borderDimColor` — common in Ink apps
5. `underline` / `inverse` — common text modifiers

**Nice to have:**
6. Individual borders (`borderTop`, etc.)
7. `transform` (uppercase/lowercase)
8. `minWidth`/`maxWidth`/`minHeight`/`maxHeight`
9. `flexBasis`/`flexGrow`/`flexShrink` from props

## Acceptance Criteria
- [ ] `backgroundColor` on Box fills background with color
- [ ] `backgroundColor` on Text sets text background
- [ ] `marginY` sets Yoga top+bottom margin
- [ ] `marginX` sets Yoga left+right margin
- [ ] `padding` creates visual padding inside ratatui Block
- [ ] All examples render without "missing prop" visual gaps

## Dependencies
- Task 025 (Box render)
- Task 026 (Text render)

## SPEC Reference
§5 ratatui Rendering
