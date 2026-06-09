# Task 029: Render Buffer Diff & Flush

## Goal
Use ratatui's double-buffered terminal output for minimal redraw.

## Acceptance Criteria
- [ ] `terminal.draw()` calls `render_yoga_tree` into ratatui `Buffer`.
- [ ] Only changed cells are flushed to terminal (ratatui handles this natively).
- [ ] Cursor hidden during draw, restored on exit.
- [ ] Integration test: two frames with single text change produce minimal ANSI diff.

## Dependencies
- Task 025

## SPEC Reference
§3.2 Renderer — Double-buffered terminal output
