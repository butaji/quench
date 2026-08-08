# Stage 25 — test/language/statements/for-of

**Status:** in_progress · **Path:** `test/language/statements/for-of`.

```bash
TEST262_STAGE=25 TEST262_DIGEST=1 TEST262_JSON=1 cargo test -p quench-runtime \
  --test test262 test262_staged -- --ignored --nocapture
```

> Note: the digest's 10s isolated-subprocess timeout flags a few heavy
> (65k-iteration) tests as timeouts even though they pass when run directly;
> treat those as timeouts, not logic failures.

## Progress log

| Date | Notes |
|------|-------|
| 2026-08-08 | Baseline: 51/751 failing. |

## Landed fixes

- Iterator close: `call_iterator_return` ran the return-method while the
  enclosing abrupt completion (break/continue/return) was still pending in
  thread-local ControlFlow, so a return-method returning an object was seen as
  returning undefined. Save/restore the enclosing completion around the close.
- IteratorNext: `take_iterator_step` treated a non-Object `next()` result as
  done instead of throwing the mandated TypeError.
- `generator.return()`: old stub marked the generator Completed without running
  finally; now resumes with a return completion that unwinds finally blocks.
- Nested-yield suspension: the walker only checked the yield signal after each
  top-level generator statement, so `try { yield; throw … }` ran the throw
  before suspending. Stop statement-list evaluation on a yield signal and let a
  suspended try body propagate without running catch/finally yet. (generator-
  close-via-* family)
- For-of: a `yield` in the loop body now suspends the loop (iterator left
  open) instead of continuing to the next iteration.

## Remaining

| Count | Family | Fix direction |
|-------|--------|---------------|
| ~15 | `yield` / `yield*` in loop body | resume re-runs from loop start, losing body position & iterator step — needs position-preserving resume |
| ~10 | dstr `…rtrn-close` | `yield` in destructuring LHS computed key; iterator close on return still wrong |
| 4 | `using` / `await using` in for-of head | explicit-resource-management not implemented |
| 4 | typedarray backed by resizable buffer | resizable ArrayBuffer not implemented |
| 2 | `iterator-as-proxy` / `iterator-next-reference` | iterator-record `next` caching (read once via getter/proxy) |
| 1 | `string-astral-truncated` | WTF-8 string storage (lone surrogates) |
| 1 | `arguments-mapped-aliasing` | mapped arguments object |