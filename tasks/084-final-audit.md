# Task 084: Final Coverage Audit — Complete TS/TSX Feature Matrix

**Priority:** P1-High
**Phase:** 9 — Final Audit
**Depends on:** 083

## Problem

There is no single document mapping every TS/TSX/React/Ink feature to its status.

## Work

### 1. Build the feature matrix

Create `docs/SUPPORTED_SUBSET.md` with a comprehensive table:

| Feature | Parser | HIR | Codegen | Example | Tests | Task |
|---------|--------|-----|---------|---------|-------|------|
| `let`/`const`/`var` | ✅ | ✅ | ✅ | ✅ | ✅ | — |
| `for` | ✅ | ✅ | ✅ | ❌ | ✅ | 042 |
| ... | ... | ... | ... | ... | ... | ... |

### 2. Categorize features

- **P0 (must work):** Core language + JSX + basic React + basic Ink
- **P1 (should work):** Advanced control flow, async, spread, nullish ops
- **P2 (nice to have):** Classes, enums, generators, decorators
- **P3 (out of scope):** `eval`, `with`, dynamic imports (compile path)

### 3. Update `tasks/index.json`

Add `coverage_matrix` field.

## Acceptance Criteria

- [ ] Coverage matrix exists in `docs/SUPPORTED_SUBSET.md`
- [ ] Every TS/TSX/React/Ink feature is mapped
- [ ] Every ❌ or ⚠️ has a linked task number
- [ ] Matrix is accurate as of the audit date
