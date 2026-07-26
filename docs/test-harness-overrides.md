# test262 Harness Overrides

**Last verified:** 2026-07-26 against upstream `tc39/test262` main.

Quench replaces several harness JS files with native Rust. These are
**workarounds for Quench incompleteness**, not upstream bugs. Remove
each when its condition is met. Track under plan B3/C1 — as `%ops%`
and the object model reach spec correctness, the native shims die.

## Done (2026-07-26)

- [x] **Deprecated verify\* stubs deleted** — `verifyWritable`,
      `verifyNotWritable`, `verifyEnumerable`, `verifyNotEnumerable`,
      `verifyConfigurable`, `verifyNotConfigurable`,
      `verifyAccessorProperty` no longer exist in Rust; the real JS
      versions from `propertyHelper.js` apply.
- [x] **Native `verifyProperty` completed** — now checks `writable`
      (with the official `isWritable` probe) and real Symbol-keyed
      enumerability; the non-official get/set identity check was
      removed.
- [x] **Native `assert_deep_equal` compares Symbol keys** (matches
      official `Reflect.ownKeys` semantics).
- [x] **`$262.evalScript` runs sloppy** (new script semantics);
      `$262.createRealm` snapshots/restores all thread-local
      intrinsics (`context/intrinsics.rs`) so sub-realms can't
      contaminate the main realm. Both die with plan B1 (Realm owns
      intrinsics).

## Remaining (in order)

- [ ] **`isConstructor.js`** — Load from disk once `Reflect.construct`
      works for all edge cases (arrows, generators, proxies). Delete
      native `is_constructor()`.
- [ ] **`assert.js`** — Fix `ValueFunction.clone()` so property writes
      on a clone propagate to the original. Then load from disk and
      delete native `assert` + `assert.sameValue` / `assert.throws` /
      `assert.compareArray` / `assert.notSameValue`.
- [ ] **`propertyHelper.js`** — Stop stripping `verifyProperty` from it
      once `Object.prototype.hasOwnProperty` and
      `Object.getOwnPropertyDescriptor` return correct descriptors for
      Symbol-keyed accessor properties. Delete native
      `verify_property()`.
- [ ] **`nonIndexNumericPropertyName` patch** — `build_script` rewrites
      `Math.pow(2, 32) - 1` to `999999` to avoid an OOM in `isWritable`
      when `array.length = 4294967295`. Fix the underlying sparse-length
      behavior (no allocation on huge length), then revert to the
      official value — the patch weakens array-index boundary checks.
- [ ] **`deepEqual.js`** — Load from disk; delete native
      `assert_deep_equal()`. Depends on: `Reflect.ownKeys`,
      `Symbol.toStringTag`, `Symbol.iterator`, `Array.isArray`, `Map`,
      `Set`, all TypedArray ctors, `Promise`, boxed primitives.
- [ ] **`detachArrayBuffer.js`** — Load from disk. Implement real
      `$262.detachArrayBuffer` (the stub only shadows `byteLength`).
- [ ] **`asyncHelpers.js`** — Load unconditionally once all async
      stages pass.
- [ ] **`$262.gc`**, **`$262.agent.*`**, **`$262.IsHTMLDDA`** —
      Implement when their stages (ArrayBuffer, Atomics, annexB) are
      reached. `IsHTMLDDA` is referenced by annexB tests and is
      currently missing entirely.

## Verification per step

```bash
cargo test -p quench-runtime -- test262::harness
TEST262_DIGEST=1 cargo test -p quench-runtime --test test262 test262_staged -- --ignored
cargo clippy -p quench-runtime --all-targets
```
