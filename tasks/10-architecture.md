# Task 10: Architecture hardening — split builtins and guard recursion

## Goal

Keep the runtime architecture healthy as it grows: split the monolithic built-ins file, finish defensive limits, and track structural debt.

## Files

- `crates/quench-runtime/src/builtins.rs`
- `crates/quench-runtime/src/interpreter.rs`
- `crates/quench-runtime/src/value.rs`
- New directory: `crates/quench-runtime/src/builtins/`

## Done

- Recursion guard is implemented in the interpreter (depth limit with a clear `JsError::StackOverflow`).

## Still to do

1. **Split `builtins.rs` into submodules.** Create:
   - `crates/quench-runtime/src/builtins/mod.rs` — registration dispatcher.
   - `crates/quench-runtime/src/builtins/array.rs` — `Array` constructor and prototype methods.
   - `crates/quench-runtime/src/builtins/map.rs` — `Map` constructor and prototype methods.
   - `crates/quench-runtime/src/builtins/set.rs` — `Set` constructor and prototype methods.
   - `crates/quench-runtime/src/builtins/promise.rs` — `Promise` constructor and prototype methods.
   - `crates/quench-runtime/src/builtins/string.rs` — `String` prototype methods.
   - `crates/quench-runtime/src/builtins/date.rs` — `Date` constructor and prototype methods.
   - `crates/quench-runtime/src/builtins/object.rs` — `Object` constructor and prototype methods.
   - `crates/quench-runtime/src/builtins/json.rs` — `JSON.stringify`/`JSON.parse`.
   - `crates/quench-runtime/src/builtins/math.rs` — `Math` object.
   - `crates/quench-runtime/src/builtins/global.rs` — globals like `setTimeout`, `parseInt`, `encodeURIComponent`.
2. **Evaluate ordered maps for object property enumeration.** If `for...in` order or JSON serialization order becomes observable, migrate `Object` internals from `std::collections::HashMap` to `indexmap` or a similar ordered map.
3. Keep behavior identical during refactoring.

## Boundaries

- Only modify `crates/quench-runtime/src/`.
- Do not touch `src/bridge/`, `src/ink/`, `src/render/`, `src/compiler/`, `src/runtime.js`, or `examples/`.

## Acceptance criteria

- `cargo check -p quench-runtime` passes after the split.
- No single file under `crates/quench-runtime/src/builtins/` exceeds 500 lines.
- A deeply nested expression (e.g., 2000 nested binary operations) returns a clear stack-overflow error instead of a Rust panic.

## Verification

```bash
cargo check -p quench-runtime
cargo test -p quench-runtime
```
