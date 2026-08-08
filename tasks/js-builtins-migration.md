# JS Builtins Migration — Rust Core gap

**Status:** in_progress · **Goal:** ADR 0001 — everything that can be done in JS
must be done in JS; the Rust core stays at the minimum possible size. Today all
builtins are Rust (`crates/quench-runtime/src/builtins/*.rs`); the target is a
self-hosted `builtins/*.js` tree calling only the `__ops__` bridge.

**Pause:** stage-25 conformance pursuit is paused while this Rust Core gap lands;
resume the stage sequence after.

## Rust Core gap (R0 + R1 infrastructure)

The core lacks the loader and bridge that let JS builtins run at scale:

- [ ] **`builtins/bootstrap.rs`** — parse + eval each embedded `builtins/*.js`
      file in dependency order during realm init (R0 item).
- [ ] **`__ops__` wired in** — `builtins/core/ops_wrapper.rs` exists but
      `register_ops_object` is never called; wire it into the init path so JS
      builtins can call spec ops (R1 item; rename global to `__ops__`).
- [ ] **`builtins/*.js` tree** — `include_str!`-embedded JS sources.
- [ ] **Proof-of-scale** — move one self-contained builtin to JS; delete its
      Rust `register_*`; gated by full `cargo test` + stage run green.

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