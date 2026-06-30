# Task 08: Verify with parity tests and example apps

## Goal

Confirm the custom runtime is fully working for the supported JS/TSX examples.

## Pareto & reuse note

- Prefer existing crates, the Rust standard library, and OS features over custom code.
- Follow the 80/20 rule: implement the subset that unblocks the targeted examples/conformance tests first.
- Defer edge cases, but document them in this task or spawn a follow-up task so they are not lost.

## Files

- `tests/parity.rs`
- `crates/quench-runtime/tests/`
- Examples: `examples/simple.js`, `examples/counter.js`, `examples/use-bridge.tsx`, `examples/animations.tsx`

## Current status

- `simple.js` is expected to work because it avoids most advanced JS features.
- `counter.js`, `use-bridge.tsx`, and `animations.tsx` are **not yet verified** as fully working. Per code inspection they depend on features that still have gaps:
  - optional chaining lowering (`config.platform?.os` in `use-bridge.tsx`)
  - destructuring function parameters (`Object.entries(config).map(([k, v]) => ...)`)
  - `arguments` object in ordinary JS-to-JS calls (used by `runtime.js` console polyfill and `createElement`)
  - `Promise.resolve`/`all` static methods on the constructor object
  - `Array.from` consuming `Set`/`Map` iterables
  - `new Array()` / `new Object()` constructor wiring
  - event-loop microtask invocation (`__tb_invoke_microtasks`, `setImmediate`, `process.nextTick`)
- The parity tests (`test_simple_js_ffi`, `test_counter_jsx_compiles`, `test_binary_exists`) are present, but full end-to-end verification is blocked until the gaps above are closed.

## Steps

1. Add unit tests in `crates/quench-runtime/tests/` for every feature closed in Tasks 01–04 and 14.
2. Run `cargo test`.
3. Run each example manually and document which ones pass/fail.
4. Fix failures in the runtime; do not modify examples.
5. Run `cargo clippy` and resolve warnings in `crates/quench-runtime/`.

## Boundaries

- `examples/` are immutable. If an example fails because of a missing engine feature, implement that feature in `crates/quench-runtime/` rather than changing the example or unrelated code.
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
