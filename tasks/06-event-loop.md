# Task 06: Integrate event loop microtasks and verify dispatch

## Goal

Make sure the event loop correctly drives JS callbacks and drains any JS microtasks that runtime.js defines.

## Files

- `src/event_loop.rs`
- `src/main.rs` (for hot-reload context setup)

## Current issues

- `runtime.js` defines `globalThis.__tb_invoke_microtasks()` but `src/event_loop.rs` never calls it. `setImmediate` and `process.nextTick` callbacks therefore never run.
- Hot reload creates a fresh `quench_runtime::Context` and loads `runtime.js` + new code, but it does not re-register the bridge host functions. The reloaded context lacks `__ink_call`, etc.

## Steps

1. In `poll_timers`, after processing timers, call `ctx.call_function("__tb_invoke_microtasks", vec![])` if the function exists.
2. In `handle_hot_reload`, after creating `new_ctx`, call the same bridge-registration helper used at startup before loading `runtime.js` and the new user code.
3. Ensure `__tb_dispatch_key`, `__tb_dispatch_mouse`, `__tb_dispatch_resize`, and `__tb_invoke_timers` are still called correctly.

## Boundaries

- Only modify `src/event_loop.rs` and, if needed, a small helper in `src/main.rs`.
- Do not change timer implementation in `src/bridge/timers.rs`, signals, or hot-reload file watcher.
- `examples/` are immutable.

## Acceptance criteria

- `cargo run -- examples/counter.js` increments the counter on timer ticks.
- Keyboard events call `__tb_dispatch_key` and update the render tree.
- `setImmediate`/`process.nextTick` callbacks are drained during the event loop.
- Hot reload re-registers bridge functions and evaluates the new file.

## Verification

```bash
cargo run -- examples/counter.js
```
