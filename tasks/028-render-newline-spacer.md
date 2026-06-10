# Task 028: Render Newline & Spacer

## Goal
Handle `ink-newline` (force line break) and `ink-spacer` (flex filler).

## Acceptance Criteria
- [ ] `Newline` renders as empty text node with height=1, width=parent.
- [ ] `Spacer` renders as invisible flex child filling remaining space.
- [ ] Both have correct Yoga measure functions.
- [ ] Unit test: Box with Text + Newline + Text stacks vertically with blank line.

## Dependencies
- Task 026

## SPEC Reference
§3 Rust Modules (ink/node.rs, render.rs)
