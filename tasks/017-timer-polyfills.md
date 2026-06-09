# Task 017: Timer Polyfills

## Goal
Bridge `setTimeout` / `setInterval` / `clearTimeout` / `clearInterval` to tokio timers.

## Acceptance Criteria
- [ ] `setTimeout(cb, ms)` → `u32` spawns tokio timer, dispatches to JS on expiry.
- [ ] `setInterval(cb, ms)` → `u32` repeats until cleared.
- [ ] `clearTimeout(id)` / `clearInterval(id)` cancels timer.
- [ ] Integration test: JS `setTimeout` callback fires exactly once; `setInterval` fires N times then cleared.

## Dependencies
- Task 013

## SPEC Reference
§2 Polyfills — setTimeout / setInterval
