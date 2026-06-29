# Task 04: Fix value and prototype model

## Goal

Make the value/prototype system consistent so built-in constructors (`Array`, `Map`, `Set`, `Date`, `String`, `Number`, `Boolean`, `Function`, `Object`) and user constructors work with `new`, method dispatch, and prototype lookup.

## Files

- `crates/quench-runtime/src/value.rs`
- `crates/quench-runtime/src/interpreter.rs`
- `crates/quench-runtime/src/builtins.rs`

## Current issues

- Built-in objects do not share a single prototype chain; each object is standalone.
- `Array` instances do not inherit from `Array.prototype`, so `[].map` is missing unless added to every array.
- `Object.prototype` is not installed on ordinary objects, breaking `Object.prototype.hasOwnProperty.call(obj, k)`.
- `Function.prototype` is not installed on function values, though `Function.prototype` methods are not heavily used.
- `new Date()` creates an object with `_timestamp` but no prototype methods.
- `new String()`, `new Number()`, `new Boolean()` are not needed but boxing should at least not crash.
- Prototype lookup in `Object::get` exists but is not wired to shared built-in prototypes.

## Steps

1. In `Context::new`, create shared prototype objects for:
   - `Object.prototype` (with `hasOwnProperty`, `toString`)
   - `Array.prototype` (with all Array methods from Task 02)
   - `Map.prototype`, `Set.prototype`, `Date.prototype`, `String.prototype`, `Function.prototype`
2. Ensure `Object::new(kind)` sets the appropriate prototype for `Array`, `Map`, `Set`, `Date`, and ordinary objects.
3. Ensure `ValueFunction::get_prototype` uses `Function.prototype` as its prototype's prototype.
4. Ensure `new Constructor(...)` looks up `Constructor.prototype`, creates an object with that prototype, calls the constructor with `this`, and returns the new object.
5. Ensure member access falls back to the prototype chain correctly.

## Boundaries

- Only modify `crates/quench-runtime/src/`.
- Do not touch `src/bridge/`, `src/ink/`, `src/render/`, `src/compiler/`.
- `examples/` are immutable.

## Acceptance criteria

- `new Map()` has `set`, `get`, `has`, `forEach`, `size`.
- `new Set()` has `add`, `has`, `forEach`, `size`.
- `[]` has `push`, `map`, `filter`, `forEach`.
- `Object.prototype.hasOwnProperty.call({a:1}, 'a')` returns `true`.
- `function Foo() { this.x = 1; }; new Foo().x` returns `1`.

## Verification

```bash
cargo test -p quench-runtime
```
