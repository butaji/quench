# Stage 20 — test/language/statements/do-while

**Status:** in_progress · **Path:** `test/language/statements/do-while`.

```bash
TEST262_STAGE=20 TEST262_DIGEST=1 TEST262_JSON=1 cargo test -p quench-runtime \
  --test test262 test262_staged -- --ignored --nocapture
```

## Progress log

| Date | Notes |
|------|-------|
| 2026-07-23 | Baseline after stage 18 |

## Top remaining clusters

| Cluster | Fix direction |
|---------|---------------|
| `ReferenceError: __in__do__IN__after__break` | do-while + for-in interaction after labeled break |
| `__odds === 0` (expected 5) | do-while loop body / completion |
| `__evaluated === undefined` | expression completion in do-while |
| `cptn-abrupt-empty` completion value | abrupt completion / empty completion |
| `tco-body.js` stack overflow | tail-call optimization in do-while body |

See `tasks/failures-20.json`.
