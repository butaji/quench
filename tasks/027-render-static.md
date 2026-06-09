# Task 027: Render Static Overlay

## Goal
Implement `ink-static` semantics: items rendered above main tree, unmounting is expensive.

## Acceptance Criteria
- [ ] Static children collected into separate overlay layer.
- [ ] Overlay rendered after main tree in `terminal.draw()`.
- [ ] Preserves Ink's "expensive unmount" semantics (batch removal).
- [ ] Unit test: Static item appears above Box in same coordinates.

## Dependencies
- Task 025

## SPEC Reference
§3.2 Renderer — InkTag::Static
