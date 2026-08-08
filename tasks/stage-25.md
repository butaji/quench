# Stage 25 — test/language/statements/for-of

**Status:** in_progress · **Path:** `test/language/statements/for-of`.

```bash
TEST262_STAGE=25 TEST262_DIGEST=1 TEST262_JSON=1 cargo test -p quench-runtime \
  --test test262 test262_staged -- --ignored --nocapture
```

> Note: the digest's default 10s isolated-subprocess timeout flags a few heavy
> (65k-iteration) tests as timeouts even though they pass with more time. For an
> accurate 0-current coverage check, run the digest with
> `TEST262_TIMEOUT_SECS=60` (stages 1 and 9 then read 100%).

> Sequencing: `tasks/index.json` is monotonic — stages 0–24 `done`, stage 25
> `pending` (current), stages 26+ `pending`. Stages 31 (return), 33 (throw), 42
> (statementList), and 57 (global) were completed early and already pass 100%;
> they are marked `pending` to keep the frontier sequential and are re-confirmed
> `done` when the run reaches them in order.

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
- For-of: suspend before the body when LHS destructuring hits a `yield` (a
  yield in a destructuring default otherwise ran the body now and on resume).
- Destructuring: don't double-evaluate a computed key containing `yield` in the
  reference-touch phase (it must be evaluated once, during assignment).

## Remaining (32)

**Dominant blocker — resume-re-execution (~25 tests):** the eval-walker generator
resumes by re-running the top-level statement from `pending_stmt`, re-executing
side effects before the yield. So a `yield` in a loop body (`i++; yield; j++;`)
re-runs `i++` and skips `j++`, and a generator suspended mid-destructuring
(`[{} = yield]`) re-steps the nested iterator on resume. This blocks the
`yield`/`yield*`-in-loop body, `yield-star`, and dstr `…rtrn-close` families.
It needs position-preserving resume (a continuation/coroutine model) — a major
architecture change, not a localized patch.

| Count | Family | Fix direction |
|-------|--------|---------------|
| ~25 | `yield`/`yield*` in loop body · dstr `…rtrn-close` | position-preserving resume (major) |
| 4 | `using` / `await using` in for-of head | explicit-resource-management not implemented |
| 4 | typedarray backed by resizable buffer | resizable ArrayBuffer not implemented |
| 2 | `iterator-as-proxy` / `iterator-next-reference` | iterator-record `next` caching (blocked by `yield*` eager materialization) |
| 1 | `string-astral-truncated` | WTF-8 string storage (lone surrogates) |
| 1 | `arguments-mapped-aliasing` | mapped arguments object |