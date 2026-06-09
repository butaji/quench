# Task 009: JS Reconciler Host Config

## Goal
Build the React reconciler host config that maps all operations to `globalThis.__ink_*`.

## Acceptance Criteria
- [ ] `createInstance(type, props)` calls `__ink_create_node`.
- [ ] `createTextInstance(text)` calls `__ink_create_text_node`.
- [ ] `appendChild`, `removeChild`, `insertBefore` call FFI equivalents.
- [ ] `commitUpdate` calls `__ink_commit_update`.
- [ ] `commitTextUpdate` calls `__ink_set_text`.
- [ ] `commitRoot` calls `__ink_commit()` once.
- [ ] Integration test: mount `<Box><Text>hi</Text></Box>` in rquickjs, verify Rust tree built.

## Dependencies
- Task 004, Task 005

## SPEC Reference
§5.1 render(); §4 FFI Protocol
