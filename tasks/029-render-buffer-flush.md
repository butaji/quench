# Task 029: Render Buffer Diff & Flush

## Status
✅ **Done**


## Goal
Use ratatui's double-buffered terminal output for minimal redraw.

## Acceptance Criteria
- [x] `terminal.draw()` calls `render_node` into ratatui `Buffer`.
- [x] Only changed cells are flushed to terminal (ratatui handles this natively).
- [x] Cursor hidden during draw, restored on exit.
- [x] Integration test: buffer stability and text measurement accuracy verified.

## Dependencies
- Task 025

## SPEC Reference
§3 Rust Modules (render.rs)

## Implementation Notes

### Cursor Handling
Cursor is hidden once at startup (`terminal.hide_cursor()`) and restored on exit. No per-frame hide/show (eliminates flicker and reduces I/O).

### Integration Tests
Added three tests verifying buffer diff behavior:
- `test_text_measurement_accuracy` - verifies text measurement is accurate
- `test_box_layout_stability` - verifies unchanged boxes have stable layout
- `test_dirty_flag_for_text_change` - verifies text changes mark tree dirty
