# Task 035: Hook useFocus & useFocusManager

## Goal
Implement focus system with tab order and focus manager.

## Acceptance Criteria
- [ ] `useFocus({isActive, autoFocus})` registers focusable node.
- [ ] `useFocusManager()` returns `{focusNext, focusPrevious, enableFocus, disableFocus}`.
- [ ] `focusNext` / `focusPrevious` cycle through registered focusable nodes.
- [ ] Only focused node receives certain keyboard shortcuts (e.g., Enter).
- [ ] Unit test: three focusable components, cycle forward/backward, verify focus state.

## Dependencies
- Task 030

## SPEC Reference
§5.3 useFocus / useFocusManager
