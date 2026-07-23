# Stage 25 — test/language/statements/for-of

**Status:** in_progress · **Path:** `test/language/statements/for-of` ·
**751 tests** · **676 pass / 75 fail (90.0%)** as of 2026-07-23.

```bash
TEST262_STAGE=25 TEST262_DIGEST=1 TEST262_JSON=1 cargo test -p quench-runtime \
  --test test262 test262_staged -- --ignored --nocapture
```

See `tasks/failures-25.json` for failure clusters.

## Recent fixes (this branch)

- **Iterator [[NextMethod]] caching:** resolve `next` once per iterator record; accessor invoked only at prologue.
- **Assignment-target ref eval:** array destructuring calls `touch_assignment_target` before `IteratorNext`.
- **Object rest prototype + string indices:** rest objects link to `%Object.prototype%`; string exotic sources expand per code unit.
- **IteratorClose throw precedence:** body throw wins over `return()` errors; `iterator_close_type_error` no longer clobbers pending `thrown_value`.
- **Object rest key order:** `copy_enumerable_own_properties` uses `enumerable_own_keys`.
- **Destructuring assign errors:** `array_with_iterator_impl` preserves assign error over close TypeError.
- **Control flow through IteratorClose / try/finally** (prior commits).
- **SetFunctionName, live iterators, Array keys/values/entries** (prior commits).

## Remaining clusters (top)

| Theme | ~count | Notes |
|------|--------|--------|
| Iterator contract (close counts) | ~15 | sameValue 0/1, 11/0 — dstr IteratorClose counting |
| yield* / yield in dstr | ~11 | sameValue 2≠1, 4≠1 |
| Resizable ArrayBuffer / TypedArray | 5 | needs `maxByteLength` + `resize` on ArrayBuffer |
| CustomError vs Error | ~4 | true≠false identity |
| Object rest on primitives | ~2 | obj-rest-number (`instanceof Object`) |
| String iteration edge cases | ~2 | astral surrogate |
| TDZ / `using` | ~4 | obj-id-init-let, head-using |

## Follow-ups before merge

- Split `eval/iteration.rs` (>500 lines) per linter R12.
- `rust-toolchain.toml` pins nightly (regress 0.11 / edition2024 deps).
