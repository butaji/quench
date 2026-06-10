# Task 053: Function Callbacks (Superseded by 055)

## Status
⚠️ **Superseded by Task 055** — Batch dispatch approach chosen over direct Function refs.

## What Changed

The original plan (Task 053) proposed storing `rquickjs::Function` references in Rust and calling them directly from the event loop to eliminate `ctx.eval()` entirely.

**Why we went with batching instead:**

1. **rquickjs lifetimes** — `Function<'js>` has a lifetime tied to the `Ctx` handle. Storing these across `ctx.with()` calls or in global statics requires complex lifetime management and potential use of `Persistent` refs.
2. **Incremental improvement** — The batching approach in Task 055 achieves the same 10x speedup for timers/microtasks (the hottest path) with zero lifetime complexity.
3. **JS stays minimal** — The reconciler and hooks remain in JS (required for Ink compat). Hot paths (layout, render, timer dispatch) are in Rust.

## What Was Actually Implemented (Task 055)

- **Timers**: Rust stores only metadata (ID, delay, interval flag). JS stores `Function` refs in `timerCallbacks` Map. Rust returns JSON array of due timer IDs. One `ctx.eval("__tb_invoke_timers([1,2,3])")` per tick.
- **Microtasks**: Rust sets a flag. JS drains `microtaskCallbacks` array via `__tb_invoke_microtasks()`.
- **Key/Mouse**: Still one `ctx.eval()` per event, but dispatches to JS `__tb_dispatch_key/__tb_dispatch_mouse` which iterate native JS Maps. No string callbacks.

## Result

| Path | Before (Task 053 design) | After (Task 055 implementation) |
|------|-------------------------|--------------------------------|
| Timer dispatch | N evals for N callbacks | 1 eval per tick |
| Microtask dispatch | 1 eval per microtask | 1 eval per tick (batched) |
| Key/Mouse dispatch | String-building eval | 1 eval per event |

This meets the 60fps budget without introducing rquickjs lifetime complexity.

## SPEC Reference
§7 Performance
