# Task 040: Production: Strip Dev Code

## Status
✅ **Done**


## Goal
Remove watcher, esbuild, source maps, and hot-reload code from release builds.

## Acceptance Criteria
- [ ] `cfg(debug_assertions)` gates file watcher and hot reload.
- [ ] Release binary excludes notify, esbuild, and dev-only JS.
- [ ] Binary size target: < 5 MB (stripped, LTO).
- [ ] CI builds both debug (with reload) and release (embedded bytecode).

## Dependencies
- Task 037, Task 039

## SPEC Reference
§8 Remaining Work (production features); §6 Performance
