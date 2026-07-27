# Stage 23 — test/language/statements/for

**Status:** done · **Path:** `test/language/statements/for`

```bash
TEST262_STAGE=23 TEST262_DIGEST=1 TEST262_JSON=1 cargo test -p quench-runtime \
  --test test262 test262_staged -- --ignored --nocapture
```

## Fixes landed

- ForInit PatternDeclaration, object destructure in C-style for init
- var hoisting, completion value, per-iteration let env, multi-decl init, TCO tail_calls_only fix

See `tasks/failures-23.json` for failure clusters.
