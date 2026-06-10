# Task 014: Keyboard Dispatch

## Goal
Serialize crossterm KeyEvent and dispatch to all registered JS input handlers.

## Acceptance Criteria
- [ ] `dispatch_key(key)` builds `{type:'key', code, modifiers, input}`.
- [ ] Calls every registered JS callback synchronously via rquickjs.
- [ ] JS handlers may call `setState`; reconciler commits before `dispatch_key` returns.
- [ ] Integration test: register handler in VM, dispatch synthetic KeyEvent, verify JS state updated.

## Dependencies
- Task 008, Task 013

## SPEC Reference
§5 Event Loop
