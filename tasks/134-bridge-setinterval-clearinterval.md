# Task 134: Add `setInterval` / `clearInterval` to rquickjs Bridge

**Priority:** P0-Critical
**Phase:** 12 — Real-World Validation
**Depends on:** 132

## Problem

The `../tui1` example uses `setInterval` and `clearInterval` for elapsed-time tracking:

```tsx
const timer = setInterval(() => {
  setElapsed(((Date.now() - startTime) / 1000).toFixed(1));
}, 100);
// ...
clearInterval(timer);
```

These are missing from the rquickjs bridge. `setTimeout` may exist (used in other examples), but `setInterval` / `clearInterval` are not confirmed.

## Bridge Implementation

```rust
// In js_bridge/mod.rs or timers module:
let timers = Object::new(ctx.clone());
timers.set("setInterval", Function::new(ctx.clone(), |cb: Function, ms: i32| {
  // Store callback in a timer map, return handle id
}))?;
timers.set("clearInterval", Function::new(ctx.clone(), |id: i32| {
  // Remove timer from map
}))?;
globals.set("__runts_timers", timers)?;
```

For `--once` mode: timers should be no-ops (the effect runs once, interval callbacks never fire). For interactive mode: timers need a real event loop integration.

## Acceptance Criteria

- [ ] `setInterval(cb, ms)` returns a handle id in rquickjs
- [ ] `clearInterval(id)` removes the interval
- [ ] `--once` mode: intervals are registered but never fire (static render)
- [ ] Interactive mode: intervals fire on the event loop
- [ ] `../tui1` example no longer throws `ReferenceError: setInterval is not defined`
