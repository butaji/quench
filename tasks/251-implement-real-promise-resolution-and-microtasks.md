# Task 251: Implement real Promise resolution and microtasks

## Status: PENDING

## Gap

Promises, `async/await`, and generators are not supported because there is no microtask queue or real Promise resolution algorithm. Built-in Promise methods are stubs or missing.

## Fix

- Implement the Promise constructor and prototype methods (`then`, `catch`, `finally`, `resolve`, `reject`, `all`, `race`, etc.).
- Add a microtask queue to the runtime event loop.
- Lay groundwork for `async/await` and generators (explicit stack or state machine).

## Acceptance criteria

- [ ] `Promise.resolve(1).then(v => v + 1)` resolves to `2`.
- [ ] `Promise.all` and `Promise.race` follow spec semantics.
- [ ] Microtasks are drained before the next macrotask.
- [ ] Regression tests for Promise resolution and microtask ordering.

## Files

- `crates/quench-runtime/src/builtins/promise.rs` (new)
- `crates/quench-runtime/src/event_loop.rs`
- `crates/quench-runtime/src/value.rs`
- `crates/quench-runtime/src/interpreter/*.rs`

## Tests unblocked

- test262 `built-ins/Promise/`
- test262 `language/expressions/async-function/`
- async/await and generator tests
