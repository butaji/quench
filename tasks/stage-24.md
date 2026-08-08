# Stage 24 — test/language/statements/for-in

**Status:** in_progress · **Path:** `test/language/statements/for-in`.

```bash
TEST262_STAGE=24 TEST262_DIGEST=1 TEST262_JSON=1 cargo test -p quench-runtime \
  --test test262 test262_staged -- --ignored --nocapture
```

## Progress log

| Date | Notes |
|------|-------|
| 2026-08-08 | Baseline: 28/115 failing. |

## Landed fixes

- Destructuring LHS (`for (let [x] in obj)`): per-iteration binding (mirrors
  for-of). (28 → 21)
- Completion value: returns body value V on end/break. (21 → 13)
- Correct key enumeration: prototype-chain walk, integer indices ascending
  then string keys in creation order, non-enumerable own props shadow
  prototype props. Object/Array.prototype methods installed non-enumerable;
  Object.defineProperty preserves absent-field attributes;
  Object.create applies descriptors. (13 → 10)

## Top remaining clusters

| Cluster | Count | Fix direction |
|---------|-------|---------------|
| `Expected ReferenceError` (TDZ) | 5 | for-in `let` binding must be in TDZ when the object expression is evaluated |
| Sputnik `A3` | 1 | strict-eval scoping of the for-in iterable assignment |
| Sputnik `A7_T2` | 1 | live enumeration (deletion during iteration must be reflected) |
| `resizable-buffer`, `head-var-bound-names-in-stmt` | 2 | for-in edge cases |