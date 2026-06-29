# Task 02: Complete Array, Map, Set, Promise, String, and Date built-ins

## Goal

Implement the standard-library surface that `runtime.js` and the Ink examples actually exercise.

## Files

- `crates/quench-runtime/src/builtins.rs` (to be split into `crates/quench-runtime/src/builtins/*.rs` as part of Task 10)
- `crates/quench-runtime/src/value.rs`

## Done

- Shared `Array.prototype` with `push`, `pop`, `shift`, `unshift`, `splice`, `slice`, `forEach`, `map`, `filter`, `find`, `indexOf`, `includes`, `join`, `flat`, `some`, `every`, `reduce`, `concat`, `reverse`, `sort`.
- `Array.isArray`, `Array.from` (arrays only), `Array.of`.
- `Map` and `Set` constructors and prototype methods (`set/get/has/delete/clear/forEach/keys/values/entries/size`).
- `String.prototype` methods including `repeat`, `padStart`, `padEnd`, `trimStart`, `trimEnd`, `replace`, `toUpperCase`, `toLowerCase`.
- `Date.prototype` getters (`getTime`, `getHours`, `getMinutes`, `getSeconds`, `getDate`, `getMonth`, `getFullYear`).
- Shared `Object.prototype` with `hasOwnProperty`, `toString`, `valueOf`.
- Real `JSON.parse` via `serde_json`.
- Native `Error`, `TypeError`, `ReferenceError`, `SyntaxError` constructors with prototypes.

## Still to do

- `Promise` (`new Promise(executor)`, `Promise.resolve`, `.then`).
- Make `Map`/`Set` iterable so `for...of` and `Array.from(mapOrSet)` work (requires `Symbol.iterator` or special-case handling).
- `Array.from` on iterables and array-like objects.
- `Date.prototype.toLocaleTimeString` (used by the runtime.js date patch).
- Shared `Function.prototype` and boxing prototypes for `String`/`Number`/`Boolean`.

## Boundaries

- Only modify `crates/quench-runtime/src/`.
- Do not change compiler output or example source.
- `examples/` are immutable.

## Acceptance criteria

- `cargo test -p quench-runtime` passes with tests for Promise, iterable Map/Set, and `Array.from`.
- `Promise.resolve(42).then(v => v)` works.
- `for (const [k, v] of new Map([['a',1]]))` iterates correctly.
- `Array.from(new Set([1,2]))` returns `[1,2]`.

## Verification

```bash
cargo test -p quench-runtime
```
