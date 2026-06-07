# Task 031: Update All Docs to Reflect rquickjs-Only Architecture

**Priority:** P2-Medium  
**Phase:** 3 — Cleanup  
**Status:** ✅ COMPLETED
**ETA:** 2–3 hours  
**Depends on:** 022, 024

## The Problem

Several docs still described the HIR interpreter as the dev engine or contained stale architecture diagrams. These misled new contributors.

## Doc Audit Results

| Document | Action | Status |
|----------|--------|--------|
| `README.md` | Updated example counts, test badge, completed tasks list | ✅ Current |
| `DESIGN.md` | Already described rquickjs correctly | ✅ Current |
| `docs/INK-ARCHITECTURE.md` | Already current | ✅ Current |
| `docs/PHILOSOPHY.md` | Already described rquickjs + Yoga accurately | ✅ Current |
| `docs/PERFORMANCE.md` | Already noted "HIR Interpreter (removed)" | ✅ Current |
| `docs/ARCHITECTURE.md` | Removed stale banner; fixed pipeline diagram ("Dev Interp" → "rquickjs"); rewrote dev path section | ✅ Fixed |
| `docs/RUNTS_COMPLETE_DESIGN.md` | Removed incorrect stale banner (content was already current) | ✅ Fixed |
| `docs/ROADMAP.md` | Fixed "Custom TSX parser" → "oxc_parser"; updated parser status section | ✅ Updated |
| `docs/MIGRATION.md` | Fixed "Custom parser" → "oxc_parser" | ✅ Fixed |
| `docs/TRANSPILATION_STRATEGY.md` | **DELETED** — entirely pre-rquickjs architecture | ✅ Removed |
| `docs/TRANSPILE_STRATEGY.md` | **DELETED** — entirely pre-rquickjs with custom recursive descent parser | ✅ Removed |
| `docs/TECHNICAL_SPEC.md` | **DELETED** — pre-rquickjs implementation draft | ✅ Removed |
| `SPEC.md` | **DELETED** — pre-rquickjs draft presenting HIR interpreter as dev engine | ✅ Removed |
| `docs/SPEC.md` | **DELETED** — pre-rquickjs spec with custom parser claims | ✅ Removed |
| `docs/DELIVERABLES.md` | **DELETED** — pre-rquickjs deliverables doc | ✅ Removed |

## Acceptance Criteria

- [x] `docs/ARCHITECTURE.md` dev path shows rquickjs (not HIR interpreter).
- [x] Zero docs present HIR interpreter as the current dev engine.
- [x] All stale pre-rquickjs docs deleted (not just bannered).
- [x] README documentation table updated to remove deleted docs.
- [x] `grep -rn "interpreter\|Interpreter" docs/ README.md DESIGN.md` returns only historical/contextual mentions (with "removed" or "was").
