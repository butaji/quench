# Stage 18 — test/language/statements/continue

**Status:** done · **Path:** `test/language/statements/continue`

```bash
TEST262_STAGE=18 TEST262_DIGEST=1 TEST262_JSON=1 cargo test -p quench-runtime \
  --test test262 test262_staged -- --ignored --nocapture
```

## Fixes landed

1. **`ControlFlow::Break/Continue(Option<String>)`** — labeled vs unlabeled targets propagate through nested loops correctly.
2. **`eval_labeled` + `eval_for`** — labeled `for` passes loop labels like `eval_do_while`; inner `while` propagates `continue label` to outer `for`.

## Reproducers kept

- `eval::statement::tests::labeled_continue::labeled_continue_to_for_from_inner_while`
