# Task 08: Verify with parity tests and example apps

## Goal

Confirm the custom runtime is fully working for the supported JS/TSX examples.

## Files

- `tests/parity.rs`
- `crates/quench-runtime/tests/` (add more unit tests as needed)
- Examples: `examples/simple.js`, `examples/counter.js`, `examples/use-bridge.tsx`, `examples/animations.tsx`

## Steps

1. Add or update tests in `tests/parity.rs`:
   - Evaluate a small JS snippet through the interpreter and assert the result.
   - Call a bridge host function from JS and assert the return value.
   - Run `examples/simple.js` and check output contains expected text.
2. Add unit tests in `crates/quench-runtime/tests/` for the features added in Tasks 01–04.
3. Run the existing parity tests:
   - `test_simple_js_ffi`
   - `test_counter_jsx_compiles`
   - `test_binary_exists`
4. Run the example apps manually:
   - `cargo run -- examples/simple.js`
   - `cargo run -- examples/counter.js`
   - `cargo run -- examples/use-bridge.tsx --prop theme=dark --prop user=admin`
   - `cargo run -- examples/animations.tsx`
5. Run `cargo clippy` and fix warnings in `crates/quench-runtime/`.

## Boundaries

- Add tests only. If an example fails because of a missing engine feature, implement that feature in `crates/quench-runtime/` rather than changing the example or unrelated code.
- Do not modify `src/bridge/`, `src/ink/`, `src/render/`, or `src/compiler/` to make tests pass.

## Acceptance criteria

- `cargo test` passes.
- `examples/simple.js` renders "Hello, Quench!" or equivalent.
- `examples/counter.js` increments a counter.
- `examples/use-bridge.tsx` renders the platform and terminal info.
- `examples/animations.tsx` renders the animation demo.
- `cargo clippy -- -W clippy::all` produces no warnings in `crates/quench-runtime/`.

## Verification

```bash
cargo test
cargo run -- examples/simple.js
cargo run -- examples/counter.js
cargo run -- examples/use-bridge.tsx --prop theme=dark --prop user=admin
cargo run -- examples/animations.tsx
cargo clippy -- -W clippy::all
```
