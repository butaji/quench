# Task 005: Bridge: Commit Pipeline

## Goal
Implement prop updates, text updates, and the commit trigger.

## Acceptance Criteria
- [ ] `__ink_commit_update(id, props)` updates props HashMap and Yoga properties.
- [ ] `__ink_set_text(id, text)` updates text content.
- [ ] `__ink_commit()` marks global dirty flag; triggers layout + render in event loop.
- [ ] Unit test: update props, call commit, verify dirty flag and updated values.

## Dependencies
- Task 004

## SPEC Reference
§3 Rust Modules (bridge/tree.rs)
