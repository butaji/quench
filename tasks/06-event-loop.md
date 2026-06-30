# Task 06: Integrate event loop microtasks and verify dispatch

**Status: COMPLETED** - Event dispatch and microtask draining work correctly.

## Goal

Make sure the event loop correctly drives JS callbacks and drains any JS microtasks that runtime.js defines.

## Pareto & reuse note

- Prefer existing crates, the Rust standard library, and OS features over custom code.
- Follow the 80/20 rule: implement the subset that unblocks the targeted examples/conformance tests first.
- Defer edge cases, but document them in this task or spawn a follow-up task so they are not lost.

## TDD & testing note

- Follow the red-green-refactor cycle: write a failing unit test first, then the minimal code to pass it, then refactor.
- Add a regression test for every bug fix and edge case covered by this task.
- Keep tests in `crates/quench-runtime/tests/` and run `cargo test -p quench-runtime` before marking work done.


## Files

- `src/event_loop.rs`
- `src/main.rs` (for hot-reload context setup)

## ✅ Completed

- ✅ `Context::call_function` falls back to `globalThis` for unresolved identifiers
- ✅ `__tb_invoke_microtasks` is called after bridge microtask drain
- ✅ `setImmediate`/`process.nextTick` are available via runtime.js
- ✅ Event handlers (`__tb_dispatch_key`, `__tb_dispatch_mouse`, `__tb_dispatch_resize`, `__tb_invoke_timers`) work correctly

## Still needed (deferred)

- ❌ Hot reload re-registers bridge functions on fresh context - partially addressed but not fully tested
- ❌ Full TTY mode rendering, mouse input, resize events - deferred to Task 08

## Acceptance criteria

- ✅ `cargo run -- examples/counter.js` increments the counter on timer ticks.
- ✅ Keyboard events call `__tb_dispatch_key` and update the render tree.
- ✅ `setImmediate`/`process.nextTick` callbacks are drained during the event loop.

## Verification

```bash
cargo run -- examples/counter.js
```
