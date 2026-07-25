# test262 harness — speed roadmap

Goal: minimize wall-clock from “failing stage” → “root cause fixed” → “100%”.
Strategy context: `tasks/10-ways-to-speed-up.md` (S2 digest, S5 harness).

## Loop we optimize

```
digest → pick largest cluster → reproducer #[test] → fix → re-digest (failed-only) → advance stage
```

Every harness change must shorten that loop or make cluster choice more accurate.

## Status (2026-07-23)

| Item | Status | Impact |
|------|--------|--------|
| Full tree collect (no silent dir skips) | **done** | Honest counts; stage 16 = 4367 files |
| Explicit `Skip` (never counted as pass) | **done** | Digest integrity |
| Parallel in-stage digest (`TEST262_PARALLEL=1` default) | **done** | ~Nx on multi-core |
| JSON digest → `tasks/failures-N.json` | **done** | Baselines, CI diff |
| Failed-only rerun (`TEST262_FAILED_JSON=…`) | **done** | Fast fix-verify |
| QUICK sample (`TEST262_QUICK=1`) | **done** | Triage new stage |
| Process isolation (`TEST262_ISOLATED=1`) | **done** | Survives SIGABRT |
| Prebuilt `run-test` (not `cargo run` per test) | **done** | Isolation 10–100× |
| Digest subprocess-by-default (`TEST262_INPROCESS=1` to opt out) | **done** | Survives stack overflow |
| 16MB test thread stack (`TEST262_INPROCESS` digest) | **done** | Deeper in-process runs |
| Crash files run isolated + `NOSKIP` in digest (not skipped) | **done** | Toward zero skips |
| `run-test` = `run_single_test` (strict/module/negative) | **done** | Debug matches harness |
| Better error normalization (strict prefix, Test262Error) | **done** | Sharper clusters |
| Crash-file path skips (10 files) | **debt** | Goal: zero skips |
| `advance-stage.sh` `$STAGE` bug | **done** | |
| Stage progress in `index.json` (`passed`/`failed`) | **todo** | Dashboard |
| Cluster filter (`TEST262_CLUSTER=ReferenceError`) | **todo** | Fix one root cause |
| Digest diff tool (`tools/diff-digest.sh`) | **todo** | Before/after PR |
| Persistent worker pool for isolation | **todo** | Medium |
| Increase thread stack / `#![recursion_limit]` for crash files | **todo** | Remove skips |
| Module + async `$DONE` in `run-test` metadata display | **done** via shared path |

## Commands (canonical)

```bash
# Full stage digest (parallel, writes JSON)
TEST262_STAGE=16 TEST262_DIGEST=1 TEST262_JSON=1 \
  cargo test -p quench-runtime --test test262 test262_staged -- --ignored --nocapture

# After fix — re-run only previous failures (~seconds vs minutes)
TEST262_STAGE=16 TEST262_DIGEST=1 TEST262_FAILED_JSON=tasks/failures-16.json \
  cargo test -p quench-runtime --test test262 test262_staged -- --ignored --nocapture

# Quick triage (top 20 failure groups)
TEST262_STAGE=16 TEST262_DIGEST=1 TEST262_QUICK=1 \
  cargo test -p quench-runtime --test test262 test262_staged -- --ignored --nocapture

# Crash-safe full stage (build run-test once first)
cargo build -p run-test
TEST262_STAGE=16 TEST262_DIGEST=1 TEST262_ISOLATED=1 \
  cargo test -p quench-runtime --test test262 test262_staged -- --ignored --nocapture

# Single test (matches harness)
cargo run -p run-test -- tests/test262/test/language/statements/class/…/test.js
```

## Env reference

| Variable | Default | Purpose |
|----------|---------|---------|
| `TEST262_STAGE` | 0 | Stage id |
| `TEST262_DIGEST` | off | Collect all failures, group |
| `TEST262_JSON` | off | Write `tasks/failures-N.json` |
| `TEST262_FAILED_JSON` | — | Rerun only paths from prior digest |
| `TEST262_QUICK` | off | Stop after N unique failure groups |
| `TEST262_QUICK_LIMIT` | 20 | Groups for QUICK |
| `TEST262_PARALLEL` | on | In-process parallel workers |
| `TEST262_SERIAL` | off | Force serial |
| `TEST262_ISOLATED` | off | Subprocess per test |
| `TEST262_NOSKIP` | off | Run crash-list files |
| `TEST262_INPROCESS` | off | Opt into fast in-process digest (unsafe on class stage) |
| `ALL_STAGES` | off | Digest/run all stages |

## Next harness priorities

1. **`TEST262_CLUSTER=<substring>`** — filter digest to one normalized group while fixing it.
2. **`tools/diff-digest.sh`** — `diff failures-16.json.before failures-16.json` → tests unlocked/regressed.
3. **`index.json` progress fields** — `last_digest: {passed, failed, skipped, at}` per stage for `stage-status.sh`.
4. **Eliminate crash skips** — larger stack in test thread or fix recursion in class/prototype paths; track skip count → 0.
5. **`digest-all.sh`** — parse new `Passed:` / `Failed:` lines; parallel stages optional (careful: hides cross-stage regressions).

## What not to build

- Parallel **stage** execution (hides regressions; rejected in S5).
- Feature skip lists (Symbol, BigInt, …) — failures must stay visible.
- Duplicating test262 assertions as unit tests.
- Hand-maintained failure markdown (JSON is source of truth).
