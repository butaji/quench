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

- Destructuring LHS: per-iteration binding (mirrors for-of). (28 → 21)
- Completion value: returns body value V on end/break. (21 → 13)
- Correct key enumeration: prototype-chain walk, indices ascending then string
  keys in creation order; Object/Array.prototype methods non-enumerable;
  defineProperty preserves absent-field attributes; Object.create applies
  descriptors. (13 → 10)
- TDZ for lexical bindings: declare in TDZ (scope confined to the for-in)
  before object eval; eval_identifier/typeof set thrown value. (10 → 5)
- Strict-eval for-in LHS: don't treat assignment-target LHS as a var. (5 → 3)
- Typed-array: zeroed elements + non-enumerable props/constructor; class
  instances no longer carry an own enumerable constructor; eval_new native
  constructor prototype for Class. (3 → 2)
- Live enumeration: skip keys deleted mid-iteration. (2 → 1)

## Remaining

| Test | Fix direction |
|------|---------------|
| `head-var-bound-names-in-stmt` | `var x;` in a for-in body block should refer to the enclosing function-scoped var, not create a shadowing block binding and reset it |