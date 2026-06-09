# Task 023: Yoga Text Measurement Bridge

## Goal
Register Rust measure function on Yoga text nodes so Yoga can size text during layout.

## Acceptance Criteria
- [ ] Text nodes get `YGNodeSetMeasureFunc` pointing to Rust callback.
- [ ] Measure func uses `unicode-width` + `textwrap` to compute `{width, height}`.
- [ ] Respects parent-provided width constraint.
- [ ] Unit test: text "Hello 世界" at max_width=10 returns correct wrapped dimensions.

## Dependencies
- Task 006, Task 003

## SPEC Reference
§3.1 Text Measurement Bridge
