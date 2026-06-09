# Task 008: FFI Input Handler Registration

## Goal
Register and unregister JS callbacks for keyboard and mouse events.

## Acceptance Criteria
- [ ] `__ink_register_input(cb)` → `u32` stores JS callback in Rust registry.
- [ ] `__ink_unregister_input(id)` removes callback.
- [ ] `__ink_register_mouse(cb)` → `u32` stores mouse callback.
- [ ] Callbacks are invoked with serialised event objects.
- [ ] Unit test: register callback, simulate dispatch, verify JS function called.

## Dependencies
- Task 001

## SPEC Reference
§4 FFI Protocol — register_input / unregister_input; §7.1 Rust → JS Dispatch
