# Task 02: Complete Array, Map, Set, Promise, String, and Date built-ins

## Goal

Implement the standard-library surface that `runtime.js` and the Ink examples actually exercise.

## Files

- `crates/quench-runtime/src/builtins/` (split into submodules)
- `crates/quench-runtime/src/value.rs`

## Done ✓

- Shared `Array.prototype` with `push`, `pop`, `shift`, `unshift`, `splice`, `slice`, `forEach`, `map`, `filter`, `find`, `indexOf`, `includes`, `join`, `flat`, `some`, `every`, `reduce`, `concat`, `reverse`, `sort`.
- `Array.isArray`, `Array.from` (arrays only), `Array.of`.
- `Map` and `Set` constructors and prototype methods (`set/get/has/delete/clear/forEach/keys/values/entries/size`).
- `String.prototype` methods including `repeat`, `padStart`, `padEnd`, `trimStart`, `trimEnd`, `replace`, `toUpperCase`, `toLowerCase`.
- `Date.prototype` getters (`getTime`, `getHours`, `getMinutes`, `getSeconds`, `getDate`, `getMonth`, `getFullYear`).
- Shared `Object.prototype` with `hasOwnProperty`, `toString`, `valueOf`.
- Real `JSON.parse` via `serde_json`.
- Native `Error`, `TypeError`, `ReferenceError`, `SyntaxError` constructors with prototypes.
- `Promise` (`new Promise(executor)`, `.then`, `.catch`, `.finally`).
- `Date.prototype.toLocaleTimeString` (used by the runtime.js date patch).
- Shared `Function.prototype` and wiring for `Function.prototype.call`/`apply`.

## Still missing / caveats

- **`Array.from` does not consume iterables** — it only clones `.elements`, so `Array.from(new Set([1,2]))` returns `[]`.
- **`Promise.resolve`/`all`/`race` are on the prototype**, not the constructor object, so `Promise.resolve(42)` does not work.
- **`new Array()` and `new Object()` are not callable** — the constructor objects lack `__call`/`constructor` wiring.
- **Map iteration order** follows `HashMap`, not insertion order.

## Acceptance criteria

- `cargo test -p quench-runtime` passes with tests for Promise, iterable Map/Set, and `Array.from`.
- `Promise.resolve(42).then(v => v)` works.
- `for (const [k, v] of new Map([['a',1]]))` iterates correctly.
- `Array.from(new Set([1,2]))` returns `[1,2]`.

## Verification

```bash
cargo test -p quench-runtime
```
