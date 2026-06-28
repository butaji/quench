# Task 06: Integrate timers and terminal events with the interpreter

## Goal

Make the event loop drive JS callbacks through the custom interpreter instead of rquickjs.

## Files

- Modify: `src/event_loop.rs`
- Modify: `src/main.rs` if signature changes are needed.

## Steps

1. Replace every `rquickjs::Context` parameter/usage in `src/event_loop.rs` with `crate::js_runtime::Context`.
2. Keep the same pending-event globals pattern but set them via `ctx.set_global`:
   - `__pending_key`, `__pending_ctrl`, `__pending_shift`, `__pending_alt`, `__pending_meta`
   - `__pending_mouse_col`, `__pending_mouse_row`, `__pending_mouse_kind`, `__pending_mouse_button`, `__pending_mouse_ctrl`, `__pending_mouse_shift`, `__pending_mouse_alt`
3. Replace function lookup/call with `ctx.call_function("__tb_dispatch_key", vec![])`, etc.
4. In `poll_timers`, after collecting fired timer IDs, call `ctx.call_function("__tb_invoke_timers", vec![Value::Array(ids)])`.
5. For hot reload, recreate a fresh interpreter context and eval the new file contents.

## Boundaries

- Only change the JS runtime interaction layer in `src/event_loop.rs`. Keep all timer, bridge, terminal, and hot-reload logic exactly as is.
- Do not modify `src/bridge/timers.rs`, `src/signals.rs`, or `src/hotreload.rs`.

## Acceptance criteria

- `cargo check` passes after removing rquickjs types from `event_loop.rs`.
- `cargo run -- examples/counter.js` counts up on timer ticks.
- Keyboard/mouse/resize events do not panic and call the matching JS dispatch function.

## Verification

```bash
cargo check
cargo run -- examples/counter.js
```
