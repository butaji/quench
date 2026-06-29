# Task 02: Complete Array, Map, Set, Promise, String, and Date built-ins

## Goal

Implement the standard-library surface that `runtime.js` and the Ink examples actually exercise.

## Files

- `crates/quench-runtime/src/builtins.rs`
- `crates/quench-runtime/src/value.rs` (for `Object::new_array_from`, prototype helpers)

## Required built-ins

### `Array`
Prototype methods used by `runtime.js` and examples:
- `push`, `pop`, `shift`, `unshift`, `splice`, `slice`
- `forEach`, `map`, `filter`, `find`, `indexOf`, `join`
- `flat`, `some`, `sort`, `concat`
- `Array.isArray`
- `Array.from` (must work with Map/Set/arrays)
- `Array.prototype.slice` (used via `.call(arguments)`)

### `Map`
- `set`, `get`, `has`, `delete`, `clear`
- `forEach`, `keys`, `values`, `entries`
- `size` getter
- `Symbol.iterator` / `for...of` support

### `Set`
- `add`, `has`, `delete`, `clear`
- `forEach`, `values`, `entries`
- `size` getter
- `Symbol.iterator` / `for...of` support

### `Promise`
- `new Promise(executor)` with `resolve`/`reject` callbacks
- `Promise.resolve(value)`
- `.then(onFulfilled)` (synchronous/thenable enough for `waitUntilExit`)

### `String.prototype`
- `repeat` (used by `animations.tsx`)
- `padStart` (used by `runtime.js` date patch)
- Existing methods (`charAt`, `indexOf`, `slice`, `split`, etc.) already partially exist; ensure they are installed on `String.prototype` and accessible via `"foo".repeat(3)`.

### `Date.prototype`
- `getTime`, `getHours`, `getMinutes`, `getSeconds`
- `toLocaleTimeString` (basic fallback is fine; `runtime.js` patches it)

### `Object.prototype`
- `hasOwnProperty` (used by `Object.prototype.hasOwnProperty.call(options, k)`)

### `JSON`
- `JSON.parse` must actually parse the string and return a JS value (currently returns the raw string).
- `JSON.stringify` already works via `serde_json`.

## Steps

1. Install `Array.prototype` on every array object (or on a shared prototype) and implement the methods above.
2. Implement `Map` and `Set` as real collections with internal storage, not empty objects with `_entries`.
3. Add a minimal `Promise` implementation that stores a value and queued `.then` callbacks.
4. Add `String.prototype.repeat`, `padStart`, and ensure other string methods live on the prototype.
5. Add `Date.prototype` methods backed by the `_timestamp` field.
6. Create a shared `Object.prototype` with `hasOwnProperty` and install it on ordinary objects.
7. Fix `JSON.parse` to deserialize into values/objects/arrays.

## Boundaries

- Only modify `crates/quench-runtime/src/`.
- Do not change compiler output or example source.

## Acceptance criteria

- `cargo test -p quench-runtime builtins` passes with tests for Array, Map, Set, Promise, String, Date, and JSON.parse.
- `[1,2,3].map(x => x*2)` evaluates to `[2,4,6]`.
- `new Map().set('a',1).get('a')` returns `1`.
- `new Set().add(1).has(1)` returns `true`.
- `Promise.resolve(42).then(v => v)` works.
- `'x'.repeat(5)` returns `'xxxxx'`.
- `JSON.parse('[1,2]')` returns an array `[1,2]`.

## Verification

```bash
cargo test -p quench-runtime builtins
```
