# Task 038: DevEx: Remount Cycle

## Goal
Implement fast unmount/eval/remount for hot reload without VM restart.

## Acceptance Criteria
- [ ] `vm.unmount_app()` destroys React root and Yoga tree.
- [ ] `ctx.eval(new_bundle)` loads updated code in same rquickjs runtime.
- [ ] `vm.mount_app()` triggers fresh React mount + `__ink_commit()`.
- [ ] Total reload latency < 50 ms measured end-to-end.
- [ ] Integration test: modify text in plugin, reload, verify new text rendered.

## Dependencies
- Task 010, Task 037, Task 002

## SPEC Reference
§6 Hot Reload
