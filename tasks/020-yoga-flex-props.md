# Task 020: Yoga Flex Props

## Goal
Map Ink flex props to Yoga node properties.

## Acceptance Criteria
- [ ] `flexDirection` → `YGDirection` (`row`, `row-reverse`, `column`, `column-reverse`).
- [ ] `justifyContent` → `YGJustify` (`flex-start`, `center`, `flex-end`, `space-between`, `space-around`).
- [ ] `alignItems` / `alignSelf` → `YGAlign`.
- [ ] `flexWrap` → `YGWrap`.
- [ ] `flexGrow` / `flexShrink` / `flexBasis` mapped.
- [ ] Unit test: each prop roundtrips through Yoga getter.

## Dependencies
- Task 003

## SPEC Reference
§3.1 Layout Engine; §9 Week 3: Layout + Widgets
