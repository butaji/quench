# Task 022: Yoga Borders & Style

## Goal
Map border props to Yoga insets and ratatui Block rendering.

## Acceptance Criteria
- [ ] `borderStyle` (`single`, `double`, `round`, `bold`, `classic`) maps to ratatui `BorderType`.
- [ ] `borderColor` / `borderDimColor` stored for render phase.
- [ ] Borders consume Yoga layout space (reducing inner content rect).
- [ ] `borderColor`, `borderDimColor` stored for render phase.
- [ ] `title`, `titleAlign` stored for Block widget.
- [ ] Unit test: Box with `borderStyle: 'round'` has smaller inner area than outer.

## Dependencies
- Task 021

## SPEC Reference
§3.1 Layout Engine; §6.2 ShadowTree Rendering
