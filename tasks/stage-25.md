# Stage 25 — test/language/statements/for-of

**Status:** in_progress · **Path:** `test/language/statements/for-of` ·
**751 tests** · **593 pass / 158 fail (78.96%)** as of 2026-07-23.

```bash
TEST262_STAGE=25 TEST262_DIGEST=1 TEST262_JSON=1 cargo test -p quench-runtime \
  --test test262 test262_staged -- --ignored --nocapture
```

See `tasks/failures-25.json` for failure clusters.

## Recent fixes (this branch)

- **Rest nested LHS lowering:** `lower_array_lhs` / `lower_object_lhs` now preserve `Rest(...)` wrappers (fixes 12+ dstr rest-nested-array cases).
- **For-of eval:** head TDZ for `let`/`const` before iterable; body completion **V** + break semantics (mirrors for-in).
- **Var+pattern hoist:** `lower_for_of_stmt` uses `lower_for_in_var_pattern_hoist` for `var` destructuring heads.
- **BindingInitialization vs assignment:** `init_to` / TDZ-safe `assign_to_identifier` (assignment throws on TDZ; for-of/for-in lexical heads use `init_to`).
- **TypedArray iteration:** `%TypedArray%.prototype[Symbol.iterator]` + `values` (fixes float32array/arguments-mapped cluster).
- **IteratorNext validation:** non-object `next()` result throws `TypeError` (fixes iterator-next-result-type / dstr *-close-null cluster).

## Remaining clusters (top)

| Theme | Notes |
|------|--------|
| Iterator close / null result type | Normal completion must call `IteratorClose`; invalid `next` results must throw |
| `yield*` / generator timeouts | Generator delegation in for-of (8 timeouts) |
| Cross-scope `var` assign in strict nested blocks | `nested.js`, some scope tests — env `set` across pushed scopes |
| `using` / `Array.prototype.keys` | Missing builtins |
| CustomError vs Error | Error subclass identity in throw paths |
