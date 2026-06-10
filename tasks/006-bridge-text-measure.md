# Task 006: Bridge: Text & Element Measure

## Status
✅ **Done**


## Goal
Implement text measurement and element measurement bridges.

## Acceptance Criteria
- [ ] `__ink_measure_text(text, max_width)` → `{width, height}` using `unicode-width` + `textwrap`.
- [ ] `__ink_measure_element(id)` → `{width, height}` from Yoga computed layout.
- [ ] Text measure unit test: multi-byte chars, wrapping, exact line counts.
- [ ] Element measure unit test: layout calculated before measure returns correct rect.

## Dependencies
- Task 003

## SPEC Reference
§3 Rust Modules (bridge/io.rs, ink/)
