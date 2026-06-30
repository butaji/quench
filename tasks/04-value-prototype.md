# Task 04: Fix value and prototype model

## Goal

Make the value/prototype system consistent so built-in constructors and user constructors work with `new`, method dispatch, and prototype lookup.

## Pareto & reuse note

- Prefer existing crates, the Rust standard library, and OS features over custom code.
- Follow the 80/20 rule: implement the subset that unblocks the targeted examples/conformance tests first.
- Defer edge cases, but document them in this task or spawn a follow-up task so they are not lost.

## Files

- `crates/quench-runtime/src/value.rs`
- `crates/quench-runtime/src/interpreter/`
- `crates/quench-runtime/src/builtins/`

## Done ✓

- Shared `Object.prototype`, `Array.prototype`, `Map.prototype`, `Set.prototype`, `Date.prototype`, and `String.prototype` are created and installed.
- `Object::new_array`, `Object::new_map`, `Object::new_set`, and ordinary object creation link to the shared prototypes.
- `New` expression evaluation looks up `Constructor.prototype`, creates an object with that prototype, calls the constructor with `this`, and returns the object.
- `ValueFunction` carries a prototype cell for user functions.
- Shared `Function.prototype` is installed on function values and wired for `Function.prototype.call`/`apply`.

## Still missing / caveats

- **`new Array()` and `new Object()` do not work** — the `Array`/`Object` constructor objects are not callable as constructors.
- **Boxing constructors** (`new String(...)`, `new Number(...)`, `new Boolean(...)`) may not behave like real boxed objects.

## Acceptance criteria

- `(function(){}).call(null, 1)` works.
- `new String("x")` does not crash and behaves like a string object.
- `Object.prototype.hasOwnProperty.call({a:1}, 'a')` returns `true`.
- `function Foo() { this.x = 1; }; new Foo().x` returns `1`.

## Verification

```bash
cargo test -p quench-runtime
```
