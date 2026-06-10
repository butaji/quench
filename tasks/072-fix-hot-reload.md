# Task 072: Fix Hot Reload — Broken Context Setup

## Status: 🔴 **CRITICAL BUG — NOT STARTED**

## Goal
Fix hot reload so that reloaded scripts actually execute in a properly initialized QuickJS context.

## Problem

`src/event_loop.rs::handle_hot_reload()` (lines ~196-220) creates a **brand new** `rquickjs::Runtime` and `Context` for the reload, but **never calls `setup_runtime()`** on it. This means:

- `__ink_call` is not registered → JS cannot talk to Rust
- `runtime.js` is not loaded → no reconciler, no hooks, no components
- Bridge config is not injected → `useBridge()` returns undefined
- Ink constants are not registered → `kittyFlags`, `kittyModifiers` missing

The reloaded script runs in a bare QuickJS VM. It silently does nothing or crashes with `__ink_call is not defined`.

## Root Cause

```rust
// event_loop.rs — handle_hot_reload
let runtime = rquickjs::Runtime::new()?;
if let Ok(ctx) = rquickjs::Context::full(&runtime) {
    ctx.with(|ctx| {
        let _ = ctx.eval::<(), _>(new_code.as_str());  // MISSING: setup_runtime(&ctx)?
    });
}
```

## Fix Approaches

### Option A: Reuse Existing Context (Recommended)
Instead of creating a new Runtime/Context, reuse the existing `js_ctx` passed into `run_event_loop()`:

1. Call a JS `unmount()` function to destroy the old React root
2. Call `__ink_destroy_root(old_root_id)` via bridge
3. `ctx.eval(new_code)` in the **same** context (already has `__ink_call`, runtime.js, etc.)
4. New root is created automatically by the script's `render()` call

### Option B: Initialize New Context Properly
If a new context is truly needed (e.g., to clear leaked JS state), call the full setup pipeline:

```rust
let runtime = rquickjs::Runtime::new()?;
let ctx = rquickjs::Context::full(&runtime)?;
setup_runtime(&ctx)?;  // Register __ink_call, load runtime.js, inject config
ctx.with(|ctx| ctx.eval(new_code))?;
```

**Trade-off:** Option A is faster (<50ms), simpler, and preserves pre-warmed JS state. Option B is cleaner but loses all timer/hook state and takes longer.

## Acceptance Criteria
- [ ] Hot reload actually re-renders the app with new code
- [ ] `examples/counter.tsx` increment logic can be modified and reloaded live
- [ ] Reload latency < 50ms end-to-end (measured from file change to new frame)
- [ ] Integration test: modify text in example, trigger reload, verify new text in output
- [ ] No `__ink_call is not defined` or `runtime.js not loaded` errors in logs
- [ ] Old root is properly destroyed before new root is created (no memory leak)

## Files to Modify
- `src/event_loop.rs` — Fix `handle_hot_reload()` to reuse context or fully initialize new one
- `src/main.rs` — Ensure `setup_runtime()` is accessible from event_loop module
- `src/hotreload.rs` — May need changes to pass existing context through

## References
- Task 037 (File Watcher)
- Task 038 (Remount Cycle)
- SPEC §11 Post-Review Remediation (hot reload sub-topic)
