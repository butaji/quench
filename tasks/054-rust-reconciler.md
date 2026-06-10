# Task 054: Rust Reconciler

## Status
❌ **Not implemented — intentionally deferred.**

## Decision

The React reconciler remains in **JavaScript** (`src/runtime.js`). It will NOT be rewritten in Rust.

## Rationale

1. **Ink compatibility** — The reconciler must support React hooks (useState, useEffect, useRef, useMemo, useCallback, useContext), component lifecycle, and JSX element trees. Moving this to Rust would require deep rquickjs integration for Function refs, hook state arrays, and closure capture — all of which are trivial in JS but complex across the FFI boundary.
2. **Not on the hot path** — The reconciler only runs when:
   - Initial render
   - State changes (setCount, setState)
   - Props change
   It does NOT run every frame. Measured at ~2ms per re-render — well within 16ms budget.
3. **Hot paths ARE in Rust** — Event loop (tokio::select!), Yoga layout (~1ms), ratatui render (~1ms), timer batch dispatch (~0.1ms) are all in Rust.
4. **Minimize JS surface** — `src/runtime.js` is ~1070 lines. This is the minimum JS required for Ink API compatibility. Everything else (layout, I/O, timers, tree storage, rendering) is Rust.

## Architecture (Actual)

```
User TS/JS → transpiled JS → rquickjs VM → runtime.js (reconciler + hooks)
                                    ↓
                           __ink_call FFI
                                    ↓
                              Rust bridge
                                    ↓
                    ┌───────────────┼───────────────┐
                    ↓               ↓               ↓
                 ink.rs        bridge.rs        main.rs
               (Yoga tree)   (timers/I/O)   (event loop + render)
```

## Performance

- Initial render: ~5ms (JS reconciler + Rust layout + render)
- Re-render: ~3-5ms (JS reconcile + Rust layout + render)
- State update to screen: ~5ms total — within 16ms 60fps budget

## Acceptance Criteria
- [x] Reconciler stays in JS for maintainability and Ink compat
- [x] Hot paths (layout, render, timers, I/O) remain in Rust
- [x] Frame budget < 16ms under load
- [x] No rquickjs lifetime complexity in reconciler

## SPEC Reference
§3 Rust Modules; §6 Performance
