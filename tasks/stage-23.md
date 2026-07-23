# Stage 23 — test/language/statements/for

**Status:** done · **Path:** `test/language/statements/for` ·
**385 tests** · **385 pass / 0 fail (100%)** as of 2026-07-23.

```bash
TEST262_STAGE=23 TEST262_DIGEST=1 TEST262_JSON=1 cargo test -p quench-runtime \
  --test test262 test262_staged -- --ignored --nocapture
```

## Progress log

| Date | Passed | Failed | % | Notes |
|------|--------|--------|---|-------|
| 2026-07-23 | **125** | **260** | **32.5%** | Baseline after stage 20 |
| 2026-07-23 | **372** | **13** | **96.6%** | ForInit PatternDeclaration, object destructure in C-style for init |
| 2026-07-23 | **385** | **0** | **100%** | var hoisting, completion value, per-iteration let env, multi-decl init, TCO tail_calls_only fix |

See `tasks/failures-23.json` for failure clusters (empty at 100%).
