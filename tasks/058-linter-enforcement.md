# Task 058: Linter Enforcement

## Goal
Add build-time lint rules to keep the Rust codebase maintainable and fast.

## Rules

| Rule | Limit | Rationale |
|------|-------|-----------|
| File length | 500 lines max | Forces module decomposition; keeps code reviewable |
| Function length | 40 lines max | Keeps functions focused and testable |
| Cyclomatic complexity | 10 max | Prevents deeply nested hot-path code that hurts 60fps targets |

## Status
🟡 **Lint rules implemented in `build.rs`, warning-only until codebase is compliant.**

## Current Violations

Running `cargo build` prints warnings like:

```
src/main.rs: exceeds 500 lines
src/bridge.rs: exceeds 500 lines
src/ink.rs: exceeds 500 lines
```

Functions flagged for length/complexity (sample):
- `main.rs::render_node` — >40 lines
- `main.rs::main` — >40 lines, complexity >10
- `main.rs::call_ink_ffi` — >40 lines
- `bridge.rs::call_ink_ffi` (if present) / various helpers

## Refactor Plan

1. **Extract render modules** from `main.rs`
   - `src/render/box.rs` — Box + border rendering
   - `src/render/text.rs` — Text paragraph rendering
   - `src/render/util.rs` — color parsing, keycode mapping
2. **Extract event loop** from `main.rs`
   - `src/app/event_loop.rs` — tokio select + dispatch
   - `src/app/startup.rs` — terminal init + argument parsing
3. **Split `bridge.rs`**
   - `src/bridge/timers.rs` — timer registry
   - `src/bridge/io.rs` — stdout/stderr/exit
   - `src/bridge/measure.rs` — text measurement
4. **Split `ink.rs`**
   - `src/ink/node.rs` — InkNode + apply_props
   - `src/ink/runtime.rs` — InkRuntime tree ops
   - `src/ink/layout.rs` — Yoga layout calc

## Enforcement

Once all files pass:
- Change `cargo:warning=` to `panic!()` in `build.rs`
- Mark this task as `done`

## Acceptance Criteria
- [ ] `cargo build` produces zero linter warnings
- [ ] All Rust source files ≤ 500 lines
- [ ] All function bodies ≤ 40 lines
- [ ] All functions complexity ≤ 10
- [ ] Build fails (not warns) on new violations

## Dependencies
- All prior Rust modules

## SPEC Reference
§3 Rust Modules
