# Task 036: Hook measureElement

## Status
✅ **Done**


## Goal
Implement `measureElement(ref)` returning Yoga-computed dimensions.

## Acceptance Criteria
- [ ] `measureElement(ref)` reads `ref.current.id`, calls `__ink_measure_element(id)`.
- [ ] Returns `{width, height}` in terminal cells.
- [ ] Returns `undefined` if ref or layout unavailable.
- [ ] Unit test: render Box, measure it, verify dimensions match Yoga layout.

## Dependencies
- Task 006, Task 010

## SPEC Reference
§3 Rust Modules (bridge/tree.rs, ink/)
