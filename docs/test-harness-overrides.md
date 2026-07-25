# test262 Harness Overrides

**Last verified:** 2026-07-25 against upstream `tc39/test262` main.

Quench skips or replaces several harness JS files with native Rust. These
are **workarounds for Quench incompleteness**, not upstream bugs. Remove
each when the condition is met.

## Checklist (do in order)

- [ ] **`isConstructor.js`** — Load from disk once `Reflect.construct`
      works for all edge cases (arrows, generators, proxies). Delete
      native `is_constructor()` in `harness/mod.rs` and in builtins code.
- [ ] **`assert.js`** — Fix `ValueFunction.clone()` so property writes on
      a clone propagate to the original. Then load from disk and delete
      native `assert` + `assert.sameValue` / `assert.throws` /
      `assert.compareArray` / `assert.notSameValue`.
- [ ] **`propertyHelper.js`** — Stop stripping `verifyProperty` from it
      once `Object.prototype.hasOwnProperty` and
      `Object.getOwnPropertyDescriptor` return correct descriptors for
      Symbol-keyed accessor properties. Delete native `verify_property()`.
- [ ] **`deepEqual.js`** — Load from disk. Delete native
      `assert_deep_equal()` in `property_helpers.rs`. Depends on:
      `Reflect.ownKeys`, `Symbol.toStringTag`, `Symbol.iterator`,
      `Array.isArray`, `Map`, `Set`, all TypedArray ctors, `Promise`,
      boxed `String`/`Number`/`Boolean`.
- [ ] **`detachArrayBuffer.js`** — Load from disk. Either implement real
      `$262.detachArrayBuffer` or remove the stub so the harness test
      gets a `ReferenceError` as it expects.
- [ ] **Deprecated verify\* stubs** — Delete six stubs from
      `property_helpers.rs` (`verifyWritable`, `verifyNotWritable`,
      `verifyEnumerable`, `verifyNotEnumerable`, `verifyConfigurable`,
      `verifyNotConfigurable`, `verifyAccessorProperty`) — they return
      `Ok(Value::Undefined)` without checking anything. Let the JS handle
      them once `propertyHelper.js` is fully loaded.
- [ ] **`asyncHelpers.js`** — Load unconditionally once all async tests pass.
- [ ] **`$262.gc`**, **`$262.detachArrayBuffer`**, **`$262.agent.*`** —
      Implement properly when their respective stages (GC, ArrayBuffer,
      Atomics) are reached. Until then the stubs are fine.

## Verification per step

```bash
TEST262_STAGE=0 cargo test -p quench-runtime --test test262 test262_staged -- --ignored --nocapture
cargo test -p quench-runtime -- test262::harness
cargo clippy -p quench-runtime --all-targets
```
