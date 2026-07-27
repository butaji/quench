# Stage 25 — test/language/statements/for-of

**Status:** in_progress · **Path:** `test/language/statements/for-of`

```bash
TEST262_STAGE=25 TEST262_DIGEST=1 TEST262_JSON=1 cargo test -p quench-runtime \
  --test test262 test262_staged -- --ignored --nocapture
```

See `tasks/failures-25.json` for failure clusters.

## Fixes landed (this branch)

- **for-of yield/yield\* resume:** `ForOfResume`/`ForOfStep`/`ForOfIteratorRun`, `body_tail_after_yield`, `eval_for_of_body_tail`; suspend on init vs body; `eval_statements` stops on generator yield.
- **Generator yield in for-of body:** suspend/resume with `ForOfSuspend` + thread-local staging.
- **Iterator `done` ToBoolean:** `take_iterator_step` uses `ToBoolean` (iterator-next-result-done-attr).
- **Arguments object iteration:** mapped getters/setters, `Symbol.iterator` factory, live index iterator.
- **Rest destructuring ref eval:** `touch_assignment_target` on rest assignment targets before step.
- **Iterator [[NextMethod]] caching:** resolve `next` once per iterator record.
- **Object rest / IteratorClose** (prior commits).

## Remaining failure clusters

See `tasks/failures-25.json` for current counts. Themes:

- IteratorClose call counting (throw-before-next in nested destructuring)
- Resizable ArrayBuffer
- SetFunctionName / fn-name in destructuring
- TDZ / using
- Binding order in destructuring
- Destructuring yield-expr (nested)
- CustomError identity
- Misc: string astral, iterator-close-null, etc.

## Follow-ups before merge

- Split `eval/iteration.rs` (>500 lines) per linter R12.
