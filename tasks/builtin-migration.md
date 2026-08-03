# Rust-to-JS Builtin Migration

Goal: keep Rust limited to the interpreter core, canonical `__ops__`, and
performance-sensitive or crate-backed primitives. Put ECMAScript algorithms,
prototype methods, constructor wiring, and property descriptors in the
self-hosted JavaScript layer.

## Rules

- Start each migration with a failing Rust refactor-pin or behavior test.
- JS implementations use canonical `__ops__` and never access Rust storage.
- Delete replaced Rust registration only after the JS path is green.
- Run the relevant Test262 stage before and after each family migration.
- Test262 digest output is the only conformance progress evidence.

## Family queue

- [ ] Complete the `__ops__` bridge and bootstrap realm lifecycle.
- [ ] Object and Reflect algorithms; retain descriptor primitives in `__ops__`.
- [ ] Array algorithms and methods; retain indexed storage and typed-array
  performance operations in Rust.
- [ ] String algorithms; retain string storage and RegExp execution in Rust.
- [ ] Number, Boolean, Symbol, and Math algorithms; retain numeric primitives.
- [ ] Map, Set, WeakMap, and WeakSet algorithms; retain keyed storage.
- [ ] Promise and iterator algorithms; retain scheduling and suspension.
- [ ] Date, JSON, URI, RegExp, BigInt, ArrayBuffer, DataView, and TypedArray
  wrappers; retain their crate-backed and raw-buffer primitives in Rust.
- [ ] Error, Function, Proxy, and remaining constructors/prototypes.
- [ ] Remove duplicate Rust registrations and dormant JS wrappers.
- [ ] Enable migrated bootstrap for normal contexts and verify all stages with
  zero skips.

## First increment

Existing JS files are evaluated only by bootstrap unit tests. Migrate one
family at a time behind that boundary, measure its Test262 stage, and delete
duplicate Rust methods only after the JS path is green.
