# Task 011: JS Component Tags

## Goal
Export Box, Text, Static, Newline, Spacer as string tags for the reconciler.

## Acceptance Criteria
- [ ] `Box = 'ink-box'`, `Text = 'ink-text'`, `Static = 'ink-static'`, `Newline = 'ink-newline'`, `Spacer = 'ink-spacer'`.
- [ ] Each tag is intercepted by host config and creates correct Rust node type.
- [ ] Unit test: import each tag, verify typeof string and correct value.

## Dependencies
- Task 009

## SPEC Reference
§5.2 Components
