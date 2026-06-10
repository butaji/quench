# Task 025: Render Box (Block)

## Status
✅ **Done**


## Goal
Render `ink-box` nodes as ratatui `Block` widgets with borders and title.

## Acceptance Criteria
- [ ] `render_yoga_tree` matches `InkTag::Box` and renders `Block` at Yoga-computed rect.
- [ ] `borderStyle` maps to `Borders` enum.
- [ ] `title` + `titleAlign` applied.
- [ ] Background color from `backgroundColor` prop.
- [ ] Unit test: render Box to in-memory Buffer, assert border characters present.

## Dependencies
- Task 024, Task 003

## SPEC Reference
§3 Rust Modules (render.rs)
