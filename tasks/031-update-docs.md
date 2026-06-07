# Task 031: Update All Docs to Reflect rquickjs-Only Architecture

**Priority:** P2-Medium  
**Phase:** 3 — Cleanup  
**Status:** ✅ COMPLETED
**ETA:** 2–3 hours  
**Depends on:** 022, 024

## The Problem

Several docs still describe the HIR interpreter as the dev engine or contain stale architecture diagrams. These mislead new contributors.

## Doc Audit Results

| Document | Status | What Was Fixed |
|----------|--------|----------------|
| `README.md` | ✅ Current | examples count updated; In-progress list updated |
| `DESIGN.md` | ✅ Current | Mentions rquickjs correctly |
| `docs/INK-ARCHITECTURE.md` | ✅ Current | Current State section updated to v4.1 |
| `docs/PHILOSOPHY.md` | ✅ Current | Already describes rquickjs + Yoga accurately |
| `docs/PERFORMANCE.md` | ✅ Current | §3.2 table already notes "HIR Interpreter (removed)" |
| `docs/ARCHITECTURE.md` | ✅ Fixed | Added stale banner; replaced "Dev Path: HIR → Interpreter" with "Dev Path: TSX → JS → rquickjs (HIR Interpreter REMOVED)"; fixed inline false claims |
| `docs/TRANSPILATION_STRATEGY.md` | ✅ Fixed | Banner added; ASCII diagram updated; §5.1 body rewritten for rquickjs; §9 table updated |
| `docs/TRANSPILE_STRATEGY.md` | ✅ Fixed | §1 pipeline updated to rquickjs; §8 title updated; §12 extensibility no longer lists interpreter |
| `docs/RUNTS_COMPLETE_DESIGN.md` | ✅ Fixed | §2.7 body rewritten to describe rquickjs; §4.2 table fixed; §5.1 test counts updated |
| `docs/ROADMAP.md` | ✅ Updated | Decision log notes HIR interpreter removed |
| `docs/TECHNICAL_SPEC.md` | ✅ Fixed | Stale banner added noting pre-rquickjs draft status |

## Steps

1. **docs/TRANSPILATION_STRATEGY.md**: Fix remaining misleading references:
   - ASCII diagram: replace "HIR → Interpreter" with "JS bundle → rquickjs"
   - §5.1 body: replace interpreter execution description with rquickjs dev path
   - §9 table: replace "Dev (Interpreter)" with "Dev (rquickjs)"
2. **docs/TECHNICAL_SPEC.md**: Add prominent stale banner noting it is a pre-rquickjs draft.
3. Run `grep -rn "interpreter\|Interpreter" docs/ README.md DESIGN.md` and verify zero misleading references.

## Acceptance Criteria

- [x] `docs/ARCHITECTURE.md` has stale banner and dev path sections rewritten.
- [x] `docs/TRANSPILATION_STRATEGY.md` ASCII diagram and §5.1/§9 no longer present interpreter as current.
- [x] `docs/TECHNICAL_SPEC.md` has stale banner.
- [x] Zero docs present HIR interpreter as the current dev engine.
- [x] All architecture diagrams show rquickjs for dev mode.
- [x] `grep -rn "interpreter\|Interpreter" docs/ README.md DESIGN.md` returns only historical/contextual mentions (with "removed" or "was").
