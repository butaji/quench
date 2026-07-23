# Stage 25 — test/language/statements/for-of

**Status:** in_progress · **Path:** `test/language/statements/for-of` ·
**751 tests** · **609 pass / 142 fail (81.1%)** as of 2026-07-23.

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
- **IteratorClose:** non-object `return()` result throws `TypeError`; **stale thrown_value** no longer blocks `return()` (for-of throw/break close).
- **Global var assignment:** sync `globalThis` on var assign; init DeclaredOnly hoists (fixes nested.js, scope tests).
- **Array.prototype iterators:** `keys`, `values`, `entries` (+ `Symbol.iterator` → values).

## Remaining clusters (top)

| Theme | Notes |
|------|--------|
| Iterator contract (sameValue 0/1) | close call counting (~15) |
| `yield*` / generator timeouts | 8 timeouts |
| Labelled break/continue from try/finally | unreachable-code tests (~12) |
| String for-of (BMP/astral) | code unit vs code point iteration |
| Resizable ArrayBuffer / TypedArray TDZ | 5 tests |
| Destructuring fn-name / rest / symbol keys | dstr subclusters |
| CustomError vs Error | Error subclass identity |
| `using` declarations | not lowered yet |
