# Task 009: JS Reconciler Host Config

## Goal
Build the React reconciler host config that targets `globalThis.__ink_*` bridge functions.

## Acceptance Criteria
- [ ] `createInstance(type, props)` calls `__ink_create_node`.
- [ ] `createTextInstance(text)` calls `__ink_create_text_node`.
- [ ] `appendChild`, `removeChild`, `insertBefore` call bridge equivalents.
- [ ] `commitUpdate` calls `__ink_commit_update`.
- [ ] `commitTextUpdate` calls `__ink_set_text`.
- [ ] `commitRoot` calls `__ink_commit()` once.
- [ ] `getPublicInstance(instance)` returns `{id: instance.id}` so refs and `measureElement` work.
- [ ] Integration test: mount `<Box><Text>hi</Text></Box>` in rquickjs, verify Rust tree built.

## Dependencies
- Task 004, Task 005

## SPEC Reference
§2 What Runs in rquickjs; §5.1 render(); §4 Bridge API
