# Task 035: Hook useFocus & useFocusManager

## Goal
Implement focus system with tab order and focus manager. Implemented entirely in JS via React context (no new Rust bridge).

## Acceptance Criteria
- [ ] `useFocus({isActive, autoFocus})` registers focusable node in a React context registry.
- [ ] `useFocusManager()` returns `{focusNext, focusPrevious, enableFocus, disableFocus}`.
- [ ] `focusNext` / `focusPrevious` cycle through registered focusable nodes.
- [ ] Only focused node receives certain keyboard shortcuts (e.g., Enter) by filtering in `useInput`.
- [ ] Unit test: three focusable components, cycle forward/backward, verify focus state.

## Dependencies
- Task 030

## SPEC Reference
§4 JS Runtime (runtime.js hooks)
