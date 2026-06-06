# Task 031: Fix Hardcoded 80×24 Terminal Dimensions in Tests and HIR Runtime

**Priority:** P2-Medium  
**Phase:** 4 — Verification & Hardening  
**ETA:** 1–2 hours  
**Depends on:** 022

## The Problem

`render_tsx(source, 80, 24)` is hardcoded in **30+ places** across tests. `useWindowSize` in HIR runtime always returns `(80, 24)`.

If deno runs in a terminal with different dimensions (e.g. 120×30), the deno output will differ from HIR output for examples that use `width`, `height`, or `useWindowSize`.

## Steps

1. Define a constant in `src/interpreter/mod.rs`:
   ```rust
   pub const DEFAULT_COLS: u16 = 80;
   pub const DEFAULT_ROWS: u16 = 24;
   ```

2. Replace all `render_tsx(src, 80, 24)` with `render_tsx(src, DEFAULT_COLS, DEFAULT_ROWS)`.

3. Update `call_use_window_size` to read actual terminal size via `crossterm::terminal::size()` when available, falling back to env vars `COLUMNS`/`LINES`, then to constants.

## Acceptance Criteria

- [ ] No hardcoded `80` or `24` in tests.
- [ ] `useWindowSize` returns actual terminal size or env var override.
- [ ] Parity harness sets `COLUMNS=80 LINES=24` for deterministic comparison.
