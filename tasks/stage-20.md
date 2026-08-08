# Stage 20 — test/language/statements/do-while

**Status:** done · **Path:** `test/language/statements/do-while`.

```bash
TEST262_STAGE=20 TEST262_DIGEST=1 TEST262_JSON=1 cargo test -p quench-runtime \
  --test test262 test262_staged -- --ignored --nocapture
```

## Progress log

| Date | Notes |
|------|-------|
| 2026-07-23 | Baseline after stage 18 |
| 2026-08-08 | 36/36 green. Fixed break/continue completion value (UpdateEmpty), String.split literal-vs-regex separator, and do-while-body tail-call detection. |

All previously-failing clusters are resolved (see notes above).
