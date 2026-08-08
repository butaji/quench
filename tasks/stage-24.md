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

- Destructuring LHS (`for (let [x] in obj)`): for-in now supports destructuring
  via per-iteration binding (mirrors for-of). let/const get a fresh per-iteration
  env; var patterns declare identifiers without destructuring the object; each
  key is destructured per iteration. (28 → 21)
- Completion value: `eval_for_in` returns the body value V on normal end and on
  break (UpdateEmpty). (21 → 13)

## Top remaining clusters

| Cluster | Count | Fix direction |
|---------|-------|---------------|
| Built-in property enumerability | 5 (`order-*`, Sputnik `A6`) | built-in prototype/static methods are installed enumerable (spec: non-enumerable); blocks enabling the correct for-in prototype-chain walk |
| `Expected ReferenceError to be thrown` (TDZ) | 5 | for-in `let` binding must be in TDZ when the object expression is evaluated |
| Sputnik `A7`, `resizable-buffer`, `head-var-bound-names-in-stmt` | 3 | for-in edge cases |