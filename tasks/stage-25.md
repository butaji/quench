# Stage 25 — test/language/statements/for-of

**Status:** in_progress · **Path:** `test/language/statements/for-of` ·
**751 tests** · **688 pass / 63 fail (91.6%)** as of 2026-07-23.

```bash
TEST262_STAGE=25 TEST262_DIGEST=1 TEST262_JSON=1 cargo test -p quench-runtime \
  --test test262 test262_staged -- --ignored --nocapture
```

See `tasks/failures-25.json` for failure clusters.

## Recent fixes (this branch)

- **Generator yield in for-of body:** suspend/resume with `ForOfSuspend` + thread-local staging; resume from statement after `yield` in block body.
- **Iterator `done` ToBoolean:** `take_iterator_step` uses `ToBoolean` (iterator-next-result-done-attr).
- **Arguments object iteration:** mapped getters/setters, `Symbol.iterator` factory, live index iterator.
- **Rest destructuring ref eval:** `touch_assignment_target` on rest assignment targets before step.
- **Iterator [[NextMethod]] caching:** resolve `next` once per iterator record.
- **Object rest / IteratorClose** (prior commits).

## Remaining clusters (~63)

| Theme | ~count | Notes |
|------|--------|--------|
| yield* / yield in dstr | ~15 | yield-star delegation, dstr yield-expr |
| IteratorClose call counting | ~8 | throw-before-next in nested dstr |
| Resizable ArrayBuffer | 5 | maxByteLength + resize |
| SetFunctionName / fn-name dstr | ~5 | obj-id-init-fn-name-* |
| TDZ / using | ~4 | obj-id-init-let, head-using |
| obj-id-init-order / evaluation | ~6 | binding order in dstr |
| CustomError identity | ~4 | |
| yield-star-from-try/catch/finally | 4 | delegate + try/finally |
| Misc | rest | string astral, iterator-close-null, etc. |

## Follow-ups before merge

- Split `eval/iteration.rs` (>500 lines) per linter R12.
- Implement proper `yield*` delegation (currently materializes / wrong suspend counts).
