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
- Every builtin algorithm, public method, constructor wiring, and property
  descriptor that can be authored in `builtins/*.js` must live there. Rust
  may retain only interpreter/core operations and implementations requiring
  Rust for performance, native memory, crate-backed functionality, or engine
  integration. A Rust implementation may also remain when an equivalent JS
  builtin would materially increase total maintained LOC; record that reason
  in the family entry.

## Family queue

- [~] The `__ops__` bridge is active and normal contexts enter
  `bootstrap_js_builtins` after native registration; realm lifecycle cleanup
  remains pending.
- [~] Object and Reflect algorithms are JS-owned where practical; descriptor,
  call, and proxy-sensitive primitives remain in `__ops__`/Rust.
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
- [~] Reflect’s complete public method surface is JS-owned over `__ops__` and
  hidden Rust call/property/descriptor primitives; proxy-sensitive execution
  remains in Rust.
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
- [~] Symbol public prototype and registry methods (`toString`, `valueOf`,
  `description`, `for`, and `keyFor`) are JS-owned over canonical primitives;
  Rust retains symbol identity, boxing, registry storage, and well-known
  symbols.
- [~] Map and Set public mutators/lookups and `forEach` are JS-owned over
  hidden Rust keyed-storage primitives; Rust retains keyed storage and
  iterator state.
- [~] `Map.groupBy` is implemented in the self-hosted JS layer over Map
  storage primitives.
- [~] WeakMap and WeakSet public methods are JS-owned over hidden Rust storage
  primitives; Rust retains weak-key storage and identity operations.
- [~] WeakRef public `deref` is JS-owned over a hidden Rust weak-reference
  primitive; FinalizationRegistry remains GC/finalizer-backed.
- [~] Iterator public static and prototype methods are JS-owned over hidden
  Rust streaming/state-machine primitives.
- [~] Generator and AsyncGenerator public methods are JS-owned wrappers over
  hidden Rust state-machine primitives; Rust retains suspension, resumption,
  completion, and async promise scheduling as interpreter execution logic.
- [~] TypedArray callback-validating methods (`filter`, `map`, `forEach`, and
  `reduce`) remain JS-owned; pass-through `fill`, `values`, `keys`, and
  `@@iterator` bindings remain Rust-owned because wrappers add LOC without an
  algorithm. Constructors, indexed storage, coercion, and iterator state stay
  Rust core.
- [~] JSON public `parse` and `stringify` methods remain Rust-owned because
  the JS layer would only add pass-through wrappers over the crate-backed
  serializer/parser, increasing total maintained LOC.
- [~] URI and numeric globals remain Rust-owned because a JS layer would only
  add pass-through wrappers over conversion, UTF-8, percent-encoding, and
  numeric parsing primitives, increasing total maintained LOC.
- [~] Promise algorithms: `Promise.prototype.catch` and `finally` are
  self-hosted in `builtins/Promise.js`; Rust retains promise state, reactions,
  and microtask scheduling. Public `then`, `resolve`, `reject`, `all`, and
  `race` are also JS-owned over hidden Rust primitives; constructor and
  combinator algorithms remain Rust-backed.
- [~] RegExp `test` and `toString` are JS-owned over hidden Rust regex
  primitives; compiled matching and `exec` remain Rust-backed.
- [~] ArrayBuffer public `slice` is JS-owned over a hidden Rust raw-buffer
  primitive. BigInt public `toString`, `valueOf`, `asIntN`, and `asUintN` are
  JS-owned over hidden Rust arbitrary-precision primitives. Date public
  static, conversion, accessor, and mutator methods are JS-owned over hidden
  Rust chrono primitives. JSON and URI public methods are JS-owned over
  crate-backed and UTF-8 primitives; DataView and remaining TypedArray
  wrappers retain their raw-buffer primitives in Rust.
- [~] Error, Function, and RegExp public methods are JS-owned where wrappers
  exist; Rust retains constructors, call mechanics, compiled matching, and
  error storage. Proxy and remaining constructors/prototypes are pending.
- [ ] Remove duplicate Rust registrations and dormant JS wrappers.
- [~] Duplicate public numeric-global registration was removed from `date.rs`;
  the URI module is now the single Rust primitive source for those wrappers.
- [~] Timer globals remain Rust-owned as host/engine integration points and
  a lower-LOC exception; their scheduling behavior is not an ECMAScript
  builtin algorithm.
- [x] Route normal context initialization through the self-hosted bootstrap
  path. Conformance polish follows the migration pass.

## First increment

Existing JS files are now routed through normal context initialization. The
migration pass moves family implementations and deletes duplicate Rust
registrations; conformance polish and stage verification follow that pass.
