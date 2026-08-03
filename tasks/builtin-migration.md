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

- [ ] Complete the `__ops__` bridge and bootstrap realm lifecycle. Normal
  contexts now enter `bootstrap_js_builtins` after native registration.
- [ ] Object and Reflect algorithms; retain descriptor primitives in `__ops__`.
- [~] `Object.is` is JS-owned through `builtins/Object.js`; Rust retains
  canonical SameValue and descriptor/constructor primitives.
- [~] `Object.keys` is JS-owned through the canonical enumerable-key
  operation; Rust retains the underlying property storage and key traversal.
- [~] `Object.values` and `Object.entries` are JS-owned over the same
  enumerable-key operation.
- [~] Object ownership/prototype/extensibility algorithms (`hasOwn`,
  `fromEntries`, prototype operations, freeze/seal queries) are JS-owned;
  Rust retains descriptor mutation primitives.
- [~] Object own-property descriptor/name queries are JS-owned; symbol-key
  enumeration remains Rust-backed until its canonical JS bridge is complete.
- [~] `Object.assign` is JS-owned over canonical key enumeration and core
  property writes.
- [~] `Object.create` is JS-owned over the core object-allocation and
  prototype primitive.
- [~] `Object.getOwnPropertySymbols` is JS-owned over the canonical own-key
  primitive; Rust retains symbol identity and key storage.
- [~] Array algorithms and methods: the JS layer now owns the common
  transformation, search, mutation, rearrangement, and accessor methods;
  Rust retains indexed storage, construction, iteration plumbing, and the
  native `toSorted` path pending polish.
- [~] `Array.isArray` is JS-owned through the canonical type predicate; Rust
  retains Array construction and indexed storage.
- [~] String algorithms: `includes`, `startsWith`, `endsWith`, `repeat`, and
  trim variants are now authored in `builtins/String.js`; Rust retains string
  storage, iteration, and RegExp execution. Remaining String methods are
  pending migration.
- [~] Math algorithms: `max`, `min`, and `abs` are self-hosted in
  `builtins/Math.js`; Rust retains numeric primitives and performance-heavy
  functions. Number/Boolean/Symbol work is pending.
- [~] Number formatting wrappers are routed through `builtins/Number.js`;
  Rust retains numeric conversion, radix formatting, and constructor/storage
  primitives until their JS algorithms have equivalent core operations.
- [~] Number static predicates (`isNaN`, `isFinite`, `isInteger`,
  `isSafeInteger`) are JS-owned; Rust retains numeric primitives.
- [~] Boolean prototype `toString` and `valueOf` are JS-owned; Rust retains
  Boolean construction, boxing, and primitive conversion.
- [~] Symbol prototype `toString` and `valueOf` are JS-owned through canonical
  `__ops__` primitives; Rust retains symbol identity, boxing, registry,
  description, and well-known symbols.
- [~] Map and Set public mutators/lookups and `forEach` are JS-owned over
  hidden Rust keyed-storage primitives; Rust retains keyed storage and
  iterator state.
- [~] WeakMap and WeakSet public methods are JS-owned over hidden Rust storage
  primitives; Rust retains weak-key storage and identity operations.
- [~] Promise algorithms: `Promise.prototype.catch` and `finally` are
  self-hosted in `builtins/Promise.js`; Rust retains promise state, reactions,
  and microtask scheduling. Constructor and combinator algorithms remain
  pending.
- [ ] Date, JSON, URI, RegExp, BigInt, ArrayBuffer, DataView, and TypedArray
  wrappers; retain their crate-backed and raw-buffer primitives in Rust.
- [ ] Error, Function, Proxy, and remaining constructors/prototypes.
- [ ] Remove duplicate Rust registrations and dormant JS wrappers.
- [x] Route normal context initialization through the self-hosted bootstrap
  path. Conformance polish follows the migration pass.

## First increment

Existing JS files are now routed through normal context initialization. The
migration pass moves family implementations and deletes duplicate Rust
registrations; conformance polish and stage verification follow that pass.
