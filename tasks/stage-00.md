# Stage 00 — test/harness

**Status:** done · **Path:** `test/harness` (116 tests)

```bash
TEST262_STAGE=0 cargo test -p quench-runtime --test test262 test262_staged -- --ignored --nocapture
```

On 100% the runner prints `ALL STAGES COMPLETE — Stage 0: 116/116`.

## Workflow

See `AGENTS.md` → "Workflow: unit tests, not guesswork". No per-stage
duplication of the rules — they are repo-wide and enforced.

## History

- assert.throws: fixed custom TypeError constructor matching (walk prototype chain)
- clippy: 0 warnings
- skips: all removed