# JS Builtins Migration — Rust Core gap

**Status:** in_progress · **Goal:** ADR 0001 — everything that can be done in JS
must be done in JS; the Rust core stays at the minimum possible size. Today all
builtins are Rust (`crates/quench-runtime/src/builtins/*.rs`); the target is a
self-hosted `builtins/*.js` tree calling only the `__ops__` bridge.

**Pause:** stage-25 conformance pursuit is paused while this Rust Core gap lands;
resume the stage sequence after.

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