# Stage 18 — test/language/statements/continue

**Status:** done · **Path:** `test/language/statements/continue` ·
**24 tests** · **24 pass / 0 fail (100%)** as of 2026-07-23.

```bash
TEST262_STAGE=18 TEST262_DIGEST=1 TEST262_JSON=1 cargo test -p quench-runtime \
  --test test262 test262_staged -- --ignored --nocapture
```

## Progress log

| Date | Passed | Failed | % | Notes |
|------|--------|--------|---|-------|
| 2026-07-23 | 23 | 1 | 95.8% | Baseline; `labeled-continue.js` infinite loop |
| 2026-07-23 | **24** | **0** | **100%** | Labeled break/continue carry target label in `ControlFlow` |

## Fixes landed

1. **`ControlFlow::Break/Continue(Option<String>)`** — labeled vs unlabeled targets propagate through nested loops correctly.
2. **`eval_labeled` + `eval_for`** — labeled `for` passes loop labels like `eval_do_while`; inner `while` propagates `continue label` to outer `for`.

## Reproducers kept

- `eval::statement::tests::labeled_continue::labeled_continue_to_for_from_inner_while`
