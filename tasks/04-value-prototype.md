# Task 04: Fix value and prototype model

## Goal

Make the value/prototype system consistent so built-in constructors and user constructors work with `new`, method dispatch, and prototype lookup.

## Files

- `crates/quench-runtime/src/value.rs`
- `crates/quench-runtime/src/interpreter.rs`
- `crates/quench-runtime/src/builtins.rs`

## Done

- Shared `Object.prototype`, `Array.prototype`, `Map.prototype`, `Set.prototype`, `Date.prototype`, and `String.prototype` are created and installed.
- `Object::new_array`, `Object::new_map`, `Object::new_set`, and ordinary object creation link to the shared prototypes.
- `New` expression evaluation looks up `Constructor.prototype`, creates an object with that prototype, calls the constructor with `this`, and returns the object.
- `ValueFunction` carries a prototype cell for user functions.

## Still to do

- Install a shared `Function.prototype` on function values and wire `Function.prototype.call`/`apply` consistently.
- Support boxing constructors: `new String(...)`, `new Number(...)`, `new Boolean(...)`.
- Ensure prototype fallback works for all object kinds after `builtins.rs` is split into submodules.

## Steps

1. Create a shared `Function.prototype` and install it on every function value.
2. Make `String`, `Number`, and `Boolean` callable as constructors that produce boxed objects (or at least primitive wrappers).
3. Add tests that exercise prototype chains for all built-in objects.

## Boundaries

- Only modify `crates/quench-runtime/src/`.
- Do not touch `src/bridge/`, `src/ink/`, `src/render/`, `src/compiler/`.
- `examples/` are immutable.

## Acceptance criteria

- `(function(){}).call(null, 1)` works.
- `new String("x")` does not crash and behaves like a string object.
- `Object.prototype.hasOwnProperty.call({a:1}, 'a')` returns `true`.
- `function Foo() { this.x = 1; }; new Foo().x` returns `1`.

## Verification

```bash
cargo test -p quench-runtime
```
