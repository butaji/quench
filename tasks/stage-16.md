# Stage 16 — test/language/statements/class

**Status:** in_progress · **Path:** `test/language/statements/class` ·
stages 00–15 done · **4,367 tests**.

```bash
TEST262_DIGEST=1 TEST262_STAGE=16 cargo test -p quench-runtime \
  --test test262 test262_staged -- --ignored --nocapture
```

On 100% the runner prints `ALL STAGES COMPLETE — Stage 16: N/N`; that
line is the gate to advance to stage 17.

## How to clear this stage (ASAP × min LOC)

Follow Phase A in `tasks/10-ways-to-speed-up.md` / `tasks/refactor-plan.md`:

1. **R4** — delete dead TComp (free LOC, no blockers).
2. **R5** — object-model spec bugs (attribute defaults, strict writes,
   ValidateAndApply, symbol identity, one lookup path). These are the
   dominant shared root causes for class failures.
3. **S2 digest** — re-run with `TEST262_DIGEST=1`, group remaining
   failures, one reproducer `#[test]` per cluster next to
   `src/eval/class*`, fix, leave the test in.
4. Grow **R1** only for ops the clusters touch. Do **not** start full
   R0 here.

Do not edit `tests/test262.rs` or anything under `tests/test262/`.

## History

_(one line per landed cluster: date — root cause — tests unlocked)_
