# Stage 25 — test/language/statements/for-of

**Status:** in_progress · **Path:** `test/language/statements/for-of` ·
**751 tests** · **604 pass / 147 fail (80.4%)** as of 2026-07-23.

```bash
TEST262_STAGE=25 TEST262_DIGEST=1 TEST262_JSON=1 cargo test -p quench-runtime \
  --test test262 test262_staged -- --ignored --nocapture
```

See `tasks/failures-25.json` for failure clusters.

## Recent fixes (this branch)

- **Rest nested LHS lowering:** `lower_array_lhs` / `lower_object_lhs` preserve `Rest(...)` wrappers.
- **For-of eval:** head TDZ, completion **V**, break semantics, `var`+pattern hoist.
- **BindingInitialization vs assignment:** `init_to` for lexical heads; assignment throws on TDZ.
- **TypedArray iteration:** `%TypedArray%.prototype[Symbol.iterator]` + `values`.
- **IteratorNext:** non-object `next()` result throws `TypeError`.
- **IteratorClose:** non-object `return()` result throws `TypeError` (8+ dstr *-close-null, iterator-close-non-object).
- **Global var assignment:** sync `globalThis` on var assign; init DeclaredOnly hoists (fixes nested.js, scope tests).

## Remaining clusters (top)

| Theme | Notes |
|------|--------|
| Iterator close on abrupt completion | return() throw propagation (~20+) |
| `yield*` / generator timeouts | 8 timeouts |
| Labelled break/continue from try/finally | unreachable-code tests (~12) |
| Iterator contract (sameValue 0/1) | close call counting (~15) |
| `using` / `Array.prototype.keys` | Missing builtins |
| CustomError vs Error | Error subclass identity |
