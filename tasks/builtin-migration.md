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
- [~] `Object.groupBy` is implemented in the self-hosted JS layer over
  iterator and object primitives.
- [~] Object own-property descriptor/name queries are JS-owned; symbol-key
  enumeration remains Rust-backed until its canonical JS bridge is complete.
- [~] `Object.assign` is JS-owned over canonical key enumeration and core
  property writes.
- [~] `Object.defineProperties` is JS-owned over the Rust descriptor
  primitive.
- [~] `Object.create` is JS-owned over the core object-allocation and
  prototype primitive.
- [~] `Object.getOwnPropertySymbols` is JS-owned over the canonical own-key
  primitive; Rust retains symbol identity and key storage.
- [~] Reflect own-key, property-presence, descriptor, prototype, and
  extensibility algorithms are JS-owned over `__ops__`.
- [~] Object prototype public methods (`toString`, `toLocaleString`,
  `valueOf`, `hasOwnProperty`, `isPrototypeOf`, and
  `propertyIsEnumerable`) are JS-owned over hidden Rust object primitives.
- [~] Array algorithms and methods: the JS layer now owns the common
  transformation, search, mutation, rearrangement, and accessor methods;
  the dormant Rust method layer has been removed; Rust retains indexed
  storage, construction, and iteration plumbing.
- [~] `Array.isArray` is JS-owned through the canonical type predicate; Rust
  retains Array construction and indexed storage.
- [~] `Array.of` is JS-owned; Rust retains the Array constructor and indexed
  storage primitives.
- [~] `Array.from` is JS-owned; Rust retains `fromAsync` and core iterator/
  storage primitives.
- [~] String algorithms: `includes`, `startsWith`, `endsWith`, `repeat`,
  `padStart`, `padEnd`, all trim variants, the public UTF-16 accessors
  (`charAt`, `charCodeAt`, `codePointAt`, `at`), `raw`, `isWellFormed`, and
  `toWellFormed` are now authored in `builtins/String.js`; Rust retains string
  storage, iteration, UTF-16 primitives, and RegExp execution. Remaining
  String methods are pending migration.
- [~] Math public algorithms and wiring are self-hosted in `builtins/Math.js`,
  including `max`, `min`, `abs`, rounding, transcendental, random, and numeric
  utility methods; Rust retains the performance-sensitive numeric primitives.
  Number/Boolean/Symbol public wiring is migrated; remaining work is spec
  surface completion and conformance polish.
- [~] Number formatting wrappers are routed through `builtins/Number.js`;
  public `toString` and `valueOf` are also JS-owned; Rust retains numeric
  conversion, radix formatting, and constructor/storage primitives.
- [~] Number static predicates (`isNaN`, `isFinite`, `isInteger`,
  `isSafeInteger`) are JS-owned; Rust retains numeric primitives.
- [~] `Number.parseInt` and `Number.parseFloat` are JS-owned aliases over
  Rust-backed global parsing primitives.
- [~] Boolean public prototype methods are JS-owned; Rust retains Boolean
  construction, boxing, and primitive conversion.
- [~] Symbol prototype `toString` and `valueOf` are JS-owned through canonical
  `__ops__` primitives; Rust retains symbol identity, boxing, registry,
  description, and well-known symbols.
- [~] Map and Set public mutators/lookups and `forEach` are JS-owned over
  hidden Rust keyed-storage primitives; Rust retains keyed storage and
  iterator state.
- [~] `Map.groupBy` is implemented in the self-hosted JS layer over Map
  storage primitives.
- [~] WeakMap and WeakSet public methods are JS-owned over hidden Rust storage
  primitives; Rust retains weak-key storage and identity operations.
- [~] Promise algorithms: `Promise.prototype.catch` and `finally` are
  self-hosted in `builtins/Promise.js`; Rust retains promise state, reactions,
  and microtask scheduling. Public `then`, `resolve`, `reject`, `all`, and
  `race` are also JS-owned over hidden Rust primitives; constructor and
  combinator algorithms remain Rust-backed.
- [~] RegExp `test` and `toString` are JS-owned over hidden Rust regex
  primitives; compiled matching and `exec` remain Rust-backed.
- [~] ArrayBuffer public `slice` is JS-owned over a hidden Rust raw-buffer
  primitive. BigInt public `toString`, `valueOf`, `asIntN`, and `asUintN` are
  JS-owned over hidden Rust arbitrary-precision primitives. Date, JSON, URI,
  DataView, and remaining TypedArray wrappers retain their crate-backed and
  raw-buffer primitives in Rust.
- [~] Error, Function, and RegExp public methods are JS-owned where wrappers
  exist; Rust retains constructors, call mechanics, compiled matching, and
  error storage. Proxy and remaining constructors/prototypes are pending.
- [ ] Remove duplicate Rust registrations and dormant JS wrappers.
- [x] Route normal context initialization through the self-hosted bootstrap
  path. Conformance polish follows the migration pass.

## First increment

Existing JS files are now routed through normal context initialization. The
migration pass moves family implementations and deletes duplicate Rust
registrations; conformance polish and stage verification follow that pass.
