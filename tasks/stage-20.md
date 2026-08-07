# Stage 20 — test/language/statements/do-while

**Status:** in_progress · **Path:** `test/language/statements/do-while` ·
**36 tests** · **29 pass / 7 fail (80.6%)** as of 2026-07-23.

```bash
TEST262_STAGE=20 TEST262_DIGEST=1 TEST262_JSON=1 cargo test -p quench-runtime \
  --test test262 test262_staged -- --ignored --nocapture
```

## Progress log

| Date | Passed | Failed | % | Notes |
|------|--------|--------|---|-------|
| 2026-07-23 | **29** | **7** | **80.6%** | Baseline after stage 18 |

## Top remaining clusters (7)

| Count | Cluster | Fix direction |
|------:|---------|---------------|
| 3 | `ReferenceError: __in__do__IN__after__break` | do-while + for-in interaction after labeled break |
| 1 | `__odds === 0` (expected 5) | do-while loop body / completion |
| 1 | `__evaluated === undefined` | expression completion in do-while |
| 1 | `cptn-abrupt-empty` completion value | abrupt completion / empty completion |
| 1 | `tco-body.js` stack overflow | tail-call optimization in do-while body |

See `tasks/failures-20.json`.
