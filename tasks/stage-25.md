# Stage 25 — test/language/statements/for-of

**Status:** in_progress · **Path:** `test/language/statements/for-of` ·
**751 tests** · **663 pass / 88 fail (88.3%)** as of 2026-07-23.

```bash
TEST262_STAGE=25 TEST262_DIGEST=1 TEST262_JSON=1 cargo test -p quench-runtime \
  --test test262 test262_staged -- --ignored --nocapture
```

See `tasks/failures-25.json` for failure clusters.

## Recent fixes (this branch)

- **Rest nested LHS lowering:** `lower_array_lhs` / `lower_object_lhs` preserve `Rest(...)` wrappers.
- **For-of eval:** head TDZ, completion **V**, break semantics, `var`+pattern hoist.
- **BindingInitialization vs assignment:** `init_to` for lexical heads; assignment throws on TDZ.
- **TypedArray iteration:** `%TypedArray%.prototype[Symbol.iterator]` + live `values`.
- **IteratorNext:** non-object `next()` result throws `TypeError`.
- **IteratorClose:** stale `thrown_value` no longer blocks `return()`; **control flow preserved** across `return()` (break/continue/return survive generator close).
- **try/finally:** abrupt completion from `finally` propagates (`restore_control_flow_after_finally`).
- **Labelled break/continue/return** from try/finally/generator for-of bodies.
- **Array.prototype iterators:** `keys`, `values`, `entries`.
- **SetFunctionName:** lexical for-of destructuring defaults (`init_to_identifier` TDZ path).
- **Live iteration:** array/TypedArray/Map/Set iterators; last-element `done` fix.

## Remaining clusters (top)

| Theme | ~count | Notes |
|------|--------|--------|
| Iterator close error precedence | ~12 | rtrn-close-err, thrw-close-err, null return |
| yield* / yield in dstr | ~11 | sameValue 2≠1, 4≠1 |
| Iterator contract (sameValue 0/1) | ~10 | close call counting |
| Resizable ArrayBuffer / TypedArray | 5 | harness `rab` TDZ / not defined |
| Object rest order / symbol keys | ~4 | obj-rest-order, number/symbol |
| TDZ / using declarations | ~4 | obj-id-init-let, head-using |
| CustomError vs Error | ~4 | true≠false identity |
| String iteration edge cases | ~2 | astral surrogate |

## Follow-ups before merge

- Split `eval/iteration.rs` (>500 lines; currently ~885) per linter R12.
