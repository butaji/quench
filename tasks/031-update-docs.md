# Task 031: Update All Docs to Reflect rquickjs-Only Architecture

**Priority:** P2-Medium  
**Phase:** 3 — Cleanup  
**ETA:** 2–3 hours  
**Depends on:** 022, 024

## The Problem

Several docs still describe the HIR interpreter as the dev engine or contain stale architecture diagrams. These mislead new contributors.

## Doc Audit Results

| Document | Status | What Needs Fixing |
|----------|--------|-----------------|
| `README.md` | ✅ Mostly current | Minor: ensure "89" → "88" examples count |
| `DESIGN.md` | ✅ Current | Mentions rquickjs correctly; check for stale claims |
| `docs/INK-ARCHITECTURE.md` | ✅ Updated | Current State section was updated in prior commit |
| `docs/PHILOSOPHY.md` | ✅ Current | Already describes rquickjs + Yoga accurately |
| `docs/PERFORMANCE.md` | ⚠️ Partially stale | §3.2 compares rquickjs vs HIR interpreter vs Cranelift — add note that HIR interpreter was removed |
| `docs/ARCHITECTURE.md` | ❌ Completely stale | Entire doc assumes HIR interpreter dev path. **Needs full rewrite or major banner.** |
| `docs/TRANSPILATION_STRATEGY.md` | ❌ Completely stale | Same — every section references interpreter. **Needs full rewrite or major banner.** |
| `docs/TRANSPILE_STRATEGY.md` | ⚠️ Partially stale | §8 and §12.6/12.7 reference interpreter; compile path is accurate. |
| `docs/RUNTS_COMPLETE_DESIGN.md` | ⚠️ Partially stale | §2.7 title was updated but body still describes interpreter capabilities. |
| `docs/ROADMAP.md` | ✅ Updated | Decision log updated in prior commit |

## Steps

1. **README.md**: Verify no stale counts or claims. Already done — confirm.
2. **docs/PERFORMANCE.md**: Add "(removed)" note to HIR interpreter column in comparison table.
3. **docs/ARCHITECTURE.md**: Either full rewrite (TSX→JS→rquickjs dev path) or add prominent stale banner and delete interpreter sections. **Recommended: add banner + delete dev path sections, keep compile path.**
4. **docs/TRANSPILATION_STRATEGY.md**: Same treatment as ARCHITECTURE.md.
5. **docs/TRANSPILE_STRATEGY.md**: Fix §8 dev mode description, remove §12.6/12.7 interpreter references.
6. **docs/RUNTS_COMPLETE_DESIGN.md**: Rewrite §2.7 body to describe rquickjs, not interpreter.
7. Run `grep -rn "interpreter\|Interpreter" docs/ README.md DESIGN.md` and verify zero misleading references.

## Acceptance Criteria

- [ ] `docs/ARCHITECTURE.md` has stale banner or is fully rewritten.
- [ ] `docs/TRANSPILATION_STRATEGY.md` has stale banner or is fully rewritten.
- [ ] Zero docs present HIR interpreter as the current dev engine.
- [ ] All architecture diagrams show rquickjs for dev mode.
- [ ] `grep -rn "interpreter\|Interpreter" docs/ README.md DESIGN.md` returns only historical/contextual mentions (with "removed" or "was").
