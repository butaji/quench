# Task 017: Timer Polyfills

## Status
✅ **Done**


## Goal
Bridge `setTimeout` / `setInterval` / `clearTimeout` / `clearInterval` to tokio timers.

## Acceptance Criteria
- [ ] `setTimeout(cb, ms)` → `u32` spawns tokio timer, dispatches to JS on expiry.
- [ ] `setInterval(cb, ms)` → `u32` repeats until cleared.
- [ ] `clearTimeout(id)` / `clearInterval(id)` cancels timer.
- [ ] Integration test: JS `setTimeout` callback fires exactly once; `setInterval` fires N times then cleared.

## Dependencies
- Task 013

> ⚠️ **Known issue:** Timer callbacks dispatch via `ctx.eval("__tb_invoke_timers()")` which can trigger JS code that calls back into Rust bridge functions. If those bridge functions attempt `borrow_mut()` on `INK_RUNTIME` while the timer system already holds a borrow, a panic occurs. See Task 087.

## SPEC Reference
§4 JS Runtime (timer/microtask polyfills)
