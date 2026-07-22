# Stage 16 — test/language/statements/class

**Status:** in_progress · **Path:** `test/language/statements/class` · stages 00–15 done.

```bash
TEST262_STAGE=16 cargo test -p quench-runtime --test test262 test262_staged -- --ignored --nocapture
```

On 100% the runner prints `ALL STAGES COMPLETE — Stage 16: N/N`; that
line is the gate to advance to stage 17.

## Workflow

See `AGENTS.md` → "Workflow: unit tests, not guesswork" — the same cycle
applies to every stage. Reproduce via a `#[test]` next to
`src/eval/class*`, watch it fail, fix minimally, verify, leave the test
in. Do not edit `tests/test262.rs` or anything under `tests/test262/`.

## History

_(add entries as fixes land)_
