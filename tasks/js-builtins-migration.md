# JS Builtins Migration — Rust Core gap

**Status:** in_progress · **Goal:** ADR 0001 — **aggressive JS-first**: default
everything to JS; the Rust core stays at the minimum possible size. Today most
builtins are Rust (`crates/quench-runtime/src/builtins/*.rs`); the target is a
self-hosted `builtins/*.js` tree calling only the `__ops__` bridge.

**Aggressive JS rule — JS unless exactly one of two exceptions holds:**
1. A JS implementation would take the **same or more LOC** than the Rust
   equivalent (Rust wins on size), or
2. It is a **very sensitive core feature** (property/value store, GC, the
   interpreter, `__ops__` itself, the parser).

When in doubt, write it in JS.

**Pause:** stage-25 conformance pursuit is paused while this Rust Core gap
lands; resume the stage sequence after.

## Rust Core gap (R0 + R1 infrastructure) — DONE

The loader and bridge that let JS builtins run at scale are landed:

- [x] **`builtins/bootstrap.rs`** — parse + eval each embedded `builtins/*.js`
      file in dependency order during realm init (R0 item).
- [x] **`__ops__` wired in** — `register_ops_object` is called at the start of
      `register_builtins`; the bridge is renamed to `__ops__` (R1 item).
- [x] **`builtins/*.js` tree** — `include_str!`-embedded JS sources
      (`builtins/core/global_functions.js`).
- [x] **Proof-of-scale** — `isNaN`/`isFinite` moved to JS over
      `__ops__.toNumber`; their Rust `register_native` bodies deleted. Full
      `cargo test` + stage 0/25 green.
- [x] **`__ops__` surface** — exposed for all-JS: `toPrimitive`, `toNumber`,
      `toPropertyKey`, `toObject`, `toString`, `sameValue`, `sameValueZero`,
      `isCallable`, `isConstructor`, `hasOwn`, `throwTypeError`. Canonical
      impls in `eval/ops.rs` (R1); unit tests pin +0/-0, NaN, callable/
      constructor invariants.
- [x] **Bootstrap ordering** — `bootstrap_js_builtins` runs after realm globals
      (`globalThis`) are set up, so JS builtins can attach to constructors.

## Open core gap — constructor-static write aliases a same-named global

`register_builtins` was called twice per realm (`Context::new()` + the explicit
call in `test262/host.rs`), so constructor globals were replaced after
bootstrap — fixed by removing the redundant re-registration in `host.rs`.

Remaining gap: a JS `Number.isNaN = …` assignment also overwrites the global
`isNaN` function (the bare `isNaN` becomes identical to `Number.isNaN`), so the
no-coercion `Number.isNaN` clobbers the coercing global `isNaN`. `Number` is
correctly the constructor (`Number !== globalThis`), and `set_static_method`
only writes the constructor's map, yet the global binding changes. This blocks
migrating constructor statics whose names collide with global functions.
`Number` statics migration was reverted pending this fix; global `isNaN`/
`isFinite` (top-level functions) migrate cleanly.

## Next: migrate the R0 order

Each builtin: JS shell → delete the Rust `register_*` → full `cargo test
-p quench-runtime` green before next.

## Migration order (R0)

`Object` → `Function` → `Error` → `Symbol` → `Number` → `Boolean` → `String`
→ `Array` → `Iterator` → `Map`/`Set`/`Weak*` → `Promise` → `JSON` →
`Reflect`/`Proxy` → `Math` → `RegExp` → `Date` → `BigInt` → TypedArray/
ArrayBuffer/DataView/Atomics → URI.

Per-builtin gate: JS shell → delete the Rust `register_*` → full `cargo test
-p quench-runtime` green before next.

## Landed

- 2026-08-08: renamed `%ops%` → `__ops__` in docs/tasks (canonical name;
  code rename is R1).