# Task 04: Add built-ins

## Goal

Provide the standard-library objects that `runtime.js` and compiled apps rely on, inside `quench-runtime`.

## Files

- Create: `crates/quench-runtime/src/builtins.rs`
- Modify: `crates/quench-runtime/src/lib.rs` to inject built-ins into every new context.

## Steps

1. Implement `builtins.rs`:
   - `console.log` -> print to stdout with `tracing::info`.
   - `JSON.stringify(value)` and `JSON.parse(string)` using `serde_json`.
   - `Object.keys`, `Object.values`, `Object.entries`, `Object.assign`.
   - `Array` constructor and prototype methods used by runtime.js: `push`, `pop`, `shift`, `unshift`, `splice`, `slice`, `forEach`, `map`, `filter`, `find`, `indexOf`, `join`, `flat`, `length` getter/setter.
   - `Map` and `Set` with `get`, `set`, `has`, `delete`, `forEach`, `size`.
   - `Math.floor`, `Math.ceil`, `Math.max`, `Math.min`, `Math.abs`.
   - `Date.now`.
   - `typeof` operator handled in interpreter for the supported types.
2. Register all built-ins as globals in `Context::new`.

## Boundaries

- Implement standard-library objects only. Do not register Ink-specific globals or bridge functions here.
- Do not modify `src/runtime.js` or compiler output.

## Acceptance criteria

- `cargo test -p quench-runtime builtins` passes.
- A JS program can create `new Map()`, `JSON.parse('[1,2]')`, and call `[1,2,3].map(x => x*2)`.

## Verification

```bash
cargo test -p quench-runtime builtins
```
