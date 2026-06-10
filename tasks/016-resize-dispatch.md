# Task 016: Terminal Resize

## Goal
Handle crossterm Resize event and recalculate Yoga root dimensions.

## Acceptance Criteria
- [ ] `dispatch_resize(w, h)` updates Yoga root width/height.
- [ ] Marks dirty so next draw recalculates layout.
- [ ] Integration test: start at 80×24, simulate resize to 120×30, verify Yoga root layout matches new size.

## Dependencies
- Task 002, Task 013

## SPEC Reference
§5 Event Loop
