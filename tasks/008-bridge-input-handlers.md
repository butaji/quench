# Task 008: Bridge: Input Handler Registration

## Goal
Register and unregister JS callbacks for keyboard and mouse events.

## Acceptance Criteria
- [ ] `__ink_register_input(cb)` → `u32` stores JS callback in Rust registry.
- [ ] `__ink_unregister_input(id)` removes callback.
- [ ] Keyboard and mouse events both dispatch through the same registered callbacks (event object has `type: 'key' | 'mouse'`).
- [ ] Callbacks are invoked with serialised event objects.
- [ ] Unit test: register callback, simulate dispatch, verify JS function called.

## Dependencies
- Task 001

## SPEC Reference
§4 Bridge API — register_input / unregister_input; §7.1 Rust → JS Dispatch
