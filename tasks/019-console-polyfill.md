# Task 019: Console Polyfill

## Status
✅ **Done**


## Goal
Bridge `console.log` / `console.error` / `console.warn` to Rust `tracing`.

## Acceptance Criteria
- [ ] `console.log(...args)` → `tracing::info!`.
- [ ] `console.error(...args)` → `tracing::error!`.
- [ ] `console.warn(...args)` → `tracing::warn!`.
- [ ] Multiple arguments joined with space.
- [ ] Integration test: JS `console.log("a", 1, true)` produces single `tracing::info` event.

## Dependencies
- Task 001

## SPEC Reference
§4 JS Runtime (console polyfill)
