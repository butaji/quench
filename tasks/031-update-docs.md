# Task 031: Update All Docs to Reflect rquickjs-Only Architecture

**Priority:** P2-Medium  
**Phase:** 3 — Cleanup  
**ETA:** 2–3 hours  
**Depends on:** 022, 024

## The Problem

Docs claim HIR interpreter is the dev engine. This is now false.

## Files to Update

- `docs/INK-ARCHITECTURE.md` — rquickjs is PRIMARY, not "future"
- `docs/PHILOSOPHY.md` — dev mode = rquickjs, not HIR interpreter
- `docs/PERFORMANCE.md` — update dev mode targets for rquickjs startup
- `docs/ARCHITECTURE.md` — dev pipeline = TSX→JS→rquickjs
- `DESIGN.md` — remove HIR interpreter trade-off claims
- `README.md` — update dev mode description

## Key Changes

**OLD:**
> Dev mode: HIR interpreter. Fastest possible reload, zero codegen dependencies.

**NEW:**
> Dev mode: TSX → JS (oxc_codegen) → rquickjs + thin Rust bridge. Full JS semantics, ~100ms startup, ~50ms reload.

**OLD:**
> HIR interpreter in dev | No native speed | Sub-100ms reload acceptable

**NEW:**
> rquickjs in dev | ~1MB binary overhead | Full JS/TSX parity, real hooks

## Acceptance Criteria

- [ ] No doc mentions HIR interpreter as dev engine.
- [ ] All docs describe rquickjs pipeline accurately.
- [ ] No stale performance claims.
