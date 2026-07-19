# Stage 00 — test/harness

**Status:** in_progress · **Path:** `test/harness` (116 tests) · 100% target before stage 01.

```bash
TEST262_STAGE=0 cargo test -p quench-runtime --test test262 test262_staged -- --ignored --nocapture
```

## First failure

#21 `assert-throws-same-realm.js` — `Expected a Test262Error, but no error was thrown.`