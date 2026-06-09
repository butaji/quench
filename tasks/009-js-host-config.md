# Task 009: JS Reconciler Host Config

## Goal
Build the React reconciler host config with TWO swappable backends:
- **Deno backend:** targets Yoga-WASM for layout + ANSI output via `Deno.stdout`.
- **Rust backend:** targets `globalThis.__ink_*` for TuiBridge.

Both share the same reconciler logic; only the host config operations differ.

## Acceptance Criteria
- [ ] `createInstance(type, props)` → creates node (WASM in Deno, `__ink_create_node` in bridge).
- [ ] `createTextInstance(text)` → creates text leaf (WASM in Deno, `__ink_create_text_node` in bridge).
- [ ] `appendChild`, `removeChild`, `insertBefore` → tree mutation in both backends.
- [ ] `commitUpdate` / `commitTextUpdate` → prop/text updates in both backends.
- [ ] `commitRoot` → triggers render (ANSI write in Deno, `__ink_commit` in bridge).
- [ ] `getPublicInstance(instance)` returns `{id: instance.id}` so refs and `measureElement` work.
- [ ] Integration test: mount `<Box><Text>hi</Text></Box>` in both Deno and rquickjs, verify tree built.

## Dependencies
- Task 004, Task 005

## SPEC Reference
§2 What Runs in rquickjs; §5.1 render(); §4 Bridge API
