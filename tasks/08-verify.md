# Task 08: Run parity tests and example apps

## Goal

Confirm the custom runtime behaves the same as the old QuickJS-based build for the supported examples.

## Files

- Create: `crates/quench-runtime/tests/` smoke tests
- Modify: `tests/parity.rs`
- Reference: `examples/simple.js`, `examples/counter.js`, `examples/animations.tsx` or similar

## Steps

1. Add unit tests in `crates/quench-runtime/tests/` that:
   - evaluate a small JS program through the interpreter and assert the result
   - exercise built-ins and closures
2. Add integration tests in `tests/parity.rs` that:
   - evaluate a JS snippet calling a bridge host function and assert the return value
3. Run the existing parity tests:
   - `test_simple_js_ffi`
   - `test_counter_jsx_compiles`
   - `test_binary_exists`
4. Run interactive examples manually and verify rendering:
   - `cargo run -- examples/simple.js`
   - `cargo run -- examples/counter.js`
   - `cargo run -- examples/animations.tsx` if supported
5. Run `cargo clippy` and fix warnings related to the new `quench-runtime` crate.

## Boundaries

- Add tests only. Do not fix example apps by changing compiler output or bridge behavior; if an example fails because the interpreter is missing a feature, add that feature to `crates/quench-runtime/`.
- Do not modify `src/bridge/`, `src/ink/`, `src/render/`, or `src/compiler/` to make tests pass.

## Acceptance criteria

- `cargo test` passes.
- `cargo run -- examples/simple.js` renders "Hello, Quench!" or equivalent output.
- `cargo run -- examples/counter.js` increments a counter.
- No warnings from `cargo clippy` in the `quench-runtime` crate.

## Verification

```bash
cargo test
cargo run -- examples/simple.js
cargo run -- examples/counter.js
cargo clippy -- -W clippy::all
```
