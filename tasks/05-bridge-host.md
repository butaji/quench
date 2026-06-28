# Task 05: Wire bridge host functions and Ink globals into the runtime

## Goal

Expose the existing Rust bridge to the interpreter so JS code can call `__ink_call`, `__ink_call_fast`, and use Ink component tags.

## Files

- Create: `src/js_runtime/host.rs`
- Modify: `src/main.rs`
- Modify or delete: `src/ink_js.rs` (currently rquickjs-specific)

## Steps

1. In `src/js_runtime/host.rs` define helper `register_ink_host_functions(ctx: &mut Context)`:
   - `__ink_call(method: string, args_json: string) -> string` forwards to `crate::bridge::call_ink_ffi`.
   - `__ink_call_fast(method_id_or_name, a, b, c, d, e) -> f64` forwards to `crate::bridge::call_ink_ffi_fast`.
2. Register globals:
   - `Box`, `Text`, `Static`, `Newline`, `Spacer` as strings (`"ink-box"`, etc.).
   - `ink` namespace object containing the same tags.
3. Replace `src/ink_js.rs` with a thin module that returns the tag constants and a `register(ctx: &mut Context)` helper, or delete it and move the constants into `host.rs`.
4. Update `src/main.rs` `setup_runtime` to:
   - create the custom `Context`
   - register host functions and globals
   - load `runtime.js`
   - inject bridge config via eval

## Boundaries

- Only add host bindings and globals. Do not change `src/bridge/ffi.rs`, `src/bridge/node.rs`, `src/bridge/tree.rs`, or any other bridge internals.
- Call existing bridge functions exactly as `src/main.rs` currently does through `rquickjs`; do not redesign the FFI contract.

## Acceptance criteria

- A JS snippet evaluated through the interpreter can call `__ink_call('create_root', '[]')` and receive `"1"`.
- `globalThis.ink.Box === 'ink-box'`.
- `cargo run -- examples/simple.js` reaches the render path (it may still fail on runtime.js features not yet implemented).

## Verification

```bash
cargo test js_runtime::host
cargo run -- examples/simple.js
```
