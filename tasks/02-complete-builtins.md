# Task 02: Complete Array, Map, Set, Promise, String, and Date built-ins

**Status: COMPLETED** - All acceptance criteria met.

## Goal

Implement the standard-library objects that Ink and the runtime.js rely on.

## TDD & testing note

- Follow the red-green-refactor cycle: write a failing unit test first, then the minimal code to pass it, then refactor.
- Add a regression test for every bug fix and edge case covered by this task.
- Keep tests in `crates/quench-runtime/tests/` and run `cargo test -p quench-runtime` before marking work done.


## Files

- `crates/quench-runtime/src/builtins/array.rs`
- `crates/quench-runtime/src/builtins/array_methods.rs`
- `crates/quench-runtime/src/builtins/map.rs`
- `crates/quench-runtime/src/builtins/promise.rs`
- `crates/quench-runtime/src/builtins/string.rs`
- `crates/quench-runtime/src/builtins/date.rs`
- `crates/quench-runtime/src/builtins/object.rs`

## ✅ Completed

- ✅ `Array.prototype.push`, `pop`, `shift`, `unshift`, `concat`, `slice`, `splice`, `indexOf`, `lastIndexOf`, `includes`, `join`, `reverse`, `forEach`, `filter`, `map`, `reduce`, `reduceRight`, `some`, `every`, `find`, `findIndex`, `flat`, `flatMap`
- ✅ `Array.from` with Set/Map/array-like iterable support
- ✅ `Array.isArray`
- ✅ `Map` and `Set` constructors with `prototype`
- ✅ `Map.prototype` methods: `set`, `get`, `has`, `delete`, `clear`, `size`, `forEach`, `keys`, `values`, `entries`
- ✅ `Set.prototype` methods: `add`, `has`, `delete`, `clear`, `size`, `forEach`, `values`, `keys`, `entries`
- ✅ Map insertion-order iteration via `_insertion_order` tracking
- ✅ `Promise` constructor with executor handling
- ✅ `Promise.prototype.then`, `catch`, `finally`
- ✅ `Promise.resolve`, `Promise.reject`, `Promise.all`, `Promise.race` on constructor
- ✅ `String.prototype` methods: `length` (getter), `charAt`, `charCodeAt`, `indexOf`, `lastIndexOf`, `toLowerCase`, `toUpperCase`, `slice`, `substring`, `substr`, `split`, `trim`, `trimStart`, `trimEnd`, `padStart`, `padEnd`, `repeat`, `startsWith`, `endsWith`, `includes`, `replace`, `search`, `match`, `matchAll`
- ✅ `Date` constructor and getters: `now`, `parse`, `getTime`, `getFullYear`, `getMonth`, `getDate`, `getDay`, `getHours`, `getMinutes`, `getSeconds`, `getMilliseconds`, `getTimezoneOffset`, `toISOString`, `toLocaleString`, `toLocaleDateString`, `toLocaleTimeString` (with options support)
- ✅ `Object` constructor with callable behavior
- ✅ `Object.prototype.hasOwnProperty`, `toString`, `valueOf`
- ✅ `Object.keys`, `values`, `entries`, `assign`, `create`, `defineProperty`, `freeze`
- ✅ `JSON.parse` and `JSON.stringify`
- ✅ `Error`, `TypeError`, `ReferenceError`, `SyntaxError` constructors
- ✅ `Function.prototype.call`, `apply`, `bind`

## Still missing (deferred)

- ❌ `Array.prototype.sort` - basic sort works; comparator support may need improvement.
- ❌ `Array.prototype.copyWithin` - not used by current examples.
- ❌ `Symbol` - not used by current examples.
- ❌ `BigInt` - deferred to Task 11 (performance).

## Acceptance criteria

- ✅ `cargo test -p quench-runtime` passes.
- ✅ `runtime.js` console polyfill works correctly.
- ✅ `Promise.resolve(42).then(v => v)` creates a Promise object.
- ✅ `Array.from(new Set([1,2]))` returns `[1,2]`.
- ✅ `new Array(1,2,3)` and `new Object()` create the expected objects.
- ✅ Map iteration maintains insertion order.

## Verification

```bash
cargo test -p quench-runtime
cargo test
```
