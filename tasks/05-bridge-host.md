# Task 05: Register bridge host functions and Ink globals from the main crate

## Goal

Expose the existing Rust bridge to the interpreter by registering host functions and Ink globals from `src/main.rs`, without letting `quench-runtime` depend on `quench` internals.

## Files

- Modify: `src/main.rs`
- Modify or delete: `src/ink_js.rs` (currently rquickjs-specific)

## Steps

1. In `quench-runtime`, ensure `Context` already exposes:
   - `register_native_function(name, callback: Box<dyn Fn(&[Value]) -> Result<Value>>)`
   - `set_global(name, value)`
   - `eval(source)`
2. In `src/main.rs` create a helper `register_ink_host_functions(ctx: &mut quench_runtime::Context)`:
   - Register `__ink_call` as a native function that forwards `(method, args_json)` to `crate::bridge::call_ink_ffi` and returns the result string.
   - Register `__ink_call_fast` as a native function that forwards `(method_id_or_name, a, b, c, d, e)` to `crate::bridge::call_ink_ffi_fast`.
3. Register globals:
   - `Box`, `Text`, `Static`, `Newline`, `Spacer` as strings (`"ink-box"`, etc.).
   - `ink` namespace object containing the same tags.
4. Replace `src/ink_js.rs` with a thin module that exports the tag constants and a `register(ctx: &mut quench_runtime::Context)` helper, or delete it and keep the constants in `src/main.rs`.
5. Update `src/main.rs` `setup_runtime` to:
   - create the `quench_runtime::Context`
   - register host functions and globals
   - load `runtime.js`
   - inject bridge config via eval

## Boundaries

- Only add host bindings and globals in the main crate. Do not change `src/bridge/ffi.rs`, `src/bridge/node.rs`, `src/bridge/tree.rs`, or any other bridge internals.
- `quench-runtime` must remain independent of `quench` bridge code; all bridge closures live in `src/main.rs`.
- Call existing bridge functions exactly as `src/main.rs` currently does through `rquickjs`; do not redesign the FFI contract.

## Acceptance criteria

- A JS snippet evaluated through the interpreter can call `__ink_call('create_root', '[]')` and receive `"1"`.
- `globalThis.ink.Box === 'ink-box'`.
- `cargo run -- examples/simple.js` reaches the render path (it may still fail on runtime.js features not yet implemented).

## Verification

```bash
cargo test -p quench-runtime
cargo run -- examples/simple.js
```
