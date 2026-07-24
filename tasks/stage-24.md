# Stage 24 — test/language/statements/for-in

**Status:** done · **Path:** `test/language/statements/for-in` ·
**115 tests** · **115 pass / 0 fail (100%)** as of 2026-07-23.

```bash
TEST262_STAGE=24 TEST262_DIGEST=1 TEST262_JSON=1 cargo test -p quench-runtime \
  --test test262 test262_staged -- --ignored --nocapture
```

## Fixes (for-in / builtins)

- `Expression::ForIn.loop_binding` + lower like for-of (let/const patterns, no iterable-at-decl)
- Unified `eval_for_in`: TDZ head scope, per-iteration let/const, completion value V, snapshot enumeration
- Prototype-chain `enumerate_for_in_keys` with shadowing; `key_still_enumerable` walks chain
- TypedArray for-in via `ObjData::Idx` length
- `Object.create` applies property descriptors; `defineProperty` preserves absent attrs on update
- Var redeclaration without initializer skips re-init; non-enumerable builtin prototype methods

See `tasks/failures-24.json` (empty at 100%).
