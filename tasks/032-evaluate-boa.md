# Task 032: Evaluate Boa as Long-Term Pure-Rust Replacement for rquickjs

**Priority:** P3-Low  
**Phase:** 3 — Future  
**ETA:** 1–2 days (spike)  
**Depends on:** 024

## The Problem

rquickjs v0.12 is aging. Boa (`boa_engine`) is pure Rust with better interop.

## Spike Goals

1. Branch `spike/boa`.
2. Replace `rquickjs` dep with `boa_engine`.
3. Rewrite `js_bridge.rs` to use Boa's `NativeFunction` API.
4. Measure:
   - Binary size delta
   - Startup time
   - Interop ergonomics (callback passing)
   - WASM compilation feasibility

## Decision Criteria

| Criterion | rquickjs | Boa | Winner |
|-----------|----------|-----|--------|
| Binary size | +~1MB | +~2MB | rquickjs |
| Startup | ~50ms | ~100ms | rquickjs |
| Pure Rust | No (C) | Yes | Boa |
| WASM | No | Yes | Boa |
| Maintenance | Stalled | Active | Boa |
| Interop | Lifetime hell | Clean | Boa |

## Acceptance Criteria

- [ ] Spike branch compiles and runs `ink-text-props`.
- [ ] Report with measurements and decision recommendation.
- [ ] Decision documented in `docs/ROADMAP.md`.
