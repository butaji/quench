# Task 015: Mouse Dispatch

## Goal
Implement mouse event hit-testing and dispatch to deepest matching node.

## Acceptance Criteria
- [ ] `dispatch_mouse(mouse)` converts crossterm MouseEvent to JS object.
- [ ] Hit-test against `computed_rect` of ShadowTree nodes.
- [ ] Dispatches to deepest node with registered mouse callback.
- [ ] Unit test: build tree with known rects, simulate click at (x,y), verify correct node receives event.

## Dependencies
- Task 008, Task 013

## SPEC Reference
§7.3 Hit Testing (Mouse)
