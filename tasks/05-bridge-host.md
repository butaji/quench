# Task 05: Fix bridge host functions and Ink globals in main crate

## Goal

Register all bridge functions that `runtime.js` and the examples call from JS, and fix the existing registrations that return wrong shapes.

## Pareto & reuse note

- Prefer existing crates, the Rust standard library, and OS features over custom code.
- Follow the 80/20 rule: implement the subset that unblocks the targeted examples/conformance tests first.
- Defer edge cases, but document them in this task or spawn a follow-up task so they are not lost.

## Files

- `src/main.rs`
- `src/ink_js.rs` (optional; constants already registered in `main.rs`)

## Missing host functions

- `__ink_get_node_parent(id)` — called by `runtime.js` mouse hit-testing. Forward to `crate::bridge::ffi::call_ink_ffi("get_node_parent", ...)`, parse the result, and return a number or `null`.
- `__ink_set_raw_mode(enabled)` — called by the `process.stdin` polyfill in `runtime.js`. Forward to `crate::bridge::ffi::call_ink_ffi("set_raw_mode", ...)`.

## Broken host functions

- `__ink_get_node_children(id)` currently creates an empty array of the right length but never fills it. Populate the elements with the parsed IDs.
- `__ink_set_timeout` and `__ink_set_interval` currently expect a function as the first argument and serialize it with `to_js_string`. Runtime.js actually calls them with a numeric JS timer ID and a delay. Verify and adjust so the first argument is treated as the callback ID/number passed to the bridge.

## Steps

1. Add `ctx.register_native("__ink_get_node_parent", ...)` in `register_bridge_functions`.
2. Add `ctx.register_native("__ink_set_raw_mode", ...)`.
3. Fix `__ink_get_node_children` to set each element of the returned array.
4. Verify `__ink_set_timeout` / `__ink_set_interval` signatures match runtime.js usage (`globalThis.__ink_set_timeout(jsId, delay)`).
5. Keep all other host functions unchanged unless they are clearly wrong.

## Boundaries

- Only modify `src/main.rs` (and optionally `src/ink_js.rs`).
- Do not change `src/bridge/` internals; only call existing `call_ink_ffi` / `call_ink_ffi_fast` methods.
- Do not touch `src/ink/`, `src/render/`, `src/compiler/`.
- `examples/` are immutable.

## Acceptance criteria

- `runtime.js` can call `__ink_get_node_parent(id)` and `__ink_set_raw_mode(bool)` without `undefined is not a function`.
- `__ink_get_node_children(rootId)` returns an array of child IDs with correct elements.
- Timer registration round-trips: `setTimeout(cb, 0)` results in `__tb_invoke_timers` being called with the matching rust timer ID.

## Verification

```bash
cargo check
cargo run -- examples/simple.js
```
