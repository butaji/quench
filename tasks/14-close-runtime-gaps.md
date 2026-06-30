# Task 14: Close remaining runtime gaps

## Goal

Fix the specific interpreter and built-in gaps that block real Ink examples from running end-to-end.

> This task collects the smaller items that remain after the major parser/lowering/built-in/prototype work in Tasks 01–04.

## Pareto & reuse note

- Prefer existing crates, the Rust standard library, and OS features over custom code.
- Follow the 80/20 rule: implement the subset that unblocks the targeted examples/conformance tests first.
- Defer edge cases, but document them in this task or spawn a follow-up task so they are not lost.

## TDD & testing note

- Follow the red-green-refactor cycle: write a failing unit test first, then the minimal code to pass it, then refactor.
- Add a regression test for every bug fix and edge case covered by this task.
- Keep tests in `crates/quench-runtime/tests/` and run `cargo test -p quench-runtime` before marking work done.


## Files

- `crates/quench-runtime/src/lower/expr.rs`
- `crates/quench-runtime/src/lower/decl.rs`
- `crates/quench-runtime/src/interpreter/call.rs`
- `crates/quench-runtime/src/interpreter/eval_expr/*.rs`
- `crates/quench-runtime/src/interpreter/eval_expr/helpers.rs`
- `crates/quench-runtime/src/builtins/array.rs`
- `crates/quench-runtime/src/builtins/promise.rs`
- `crates/quench-runtime/src/builtins/map.rs`
- `crates/quench-runtime/src/value/mod.rs`
- `src/event_loop.rs`
- `src/runtime.js`

## ✅ Completed gaps

1. **Optional chaining lowering** (`obj?.prop`, `obj?.[expr]`, `obj?.()`)
   - ✅ `lower/expr.rs` implements `lower_opt_chain` that lowers to conditional expressions.
   - `config.platform?.os` evaluates safely (returns `undefined` if `platform` is missing).

2. **Destructuring assignment**
   - ✅ `lower/expr.rs` implements `lower_destructuring_assign` for array/object patterns.
   - `Object.entries(config).map(([k, v]) => ...)` binds `k` and `v` correctly.

3. **Destructuring function/arrow parameters**
   - ✅ `lower/decl.rs` handles destructuring params via `expand_nested_pattern`.

4. **Rest parameters in arrow functions**
   - ✅ `lower/expr.rs` captures rest params in `lower_arrow_expr`.

5. **`arguments` object in JS-to-JS calls**
   - ✅ `interpreter/call.rs` now sets up `arguments` object in call environment.
   - `runtime.js` console polyfill and `createElement` can read `arguments`.

6. **`Promise` static methods on the constructor**
   - ✅ `builtins/promise.rs` installs `resolve`, `reject`, `all`, `race` on Promise constructor.

7. **`Array.from` iterable support**
   - ✅ `builtins/array.rs` handles Set/Map/array-like iterables in `Array.from`.

8. **Callable `Array`/`Object` constructors**
   - ✅ Both constructors have `__call` handlers in their respective builtin modules.

9. **Event-loop microtask invocation**
   - ✅ `src/runtime.js` provides `setImmediate`, `process.nextTick` via microtaskCallbacks queue.
   - ✅ `src/event_loop.rs` now calls `__tb_invoke_microtasks` when microtasks are pending.
   - ✅ `eval_identifier` in `helpers.rs` now falls back to `globalThis` for unresolved identifiers.

10. **Map insertion-order iteration**
    - ✅ `builtins/map.rs` maintains `_insertion_order` array for keys.

## Remaining gaps (deferred to future tasks)

- Class expressions/statements
- `delete` operator
- `yield` / generators
- `async`/`await` syntax
- ES modules (`import`/`export`)

## Boundaries

- Only modify `crates/quench-runtime/src/` and `src/event_loop.rs`.
- Do not touch `src/bridge/` internals beyond the existing `__ink_enqueue_microtask` host call.
- `examples/` are immutable; fix the runtime to make them pass.

## Acceptance criteria

- ✅ `config.platform?.os` evaluates safely (returns `undefined` if `platform` is missing).
- ✅ `Object.entries(config).map(([k, v]) => ...)` binds `k` and `v` correctly.
- ✅ `runtime.js` console polyfill and `createElement` can read `arguments`.
- ✅ `Promise.resolve(42).then(v => v)` creates a Promise object.
- ✅ `Array.from(new Set([1,2]))` returns `[1,2]`.
- ✅ `new Array(1,2,3)` and `new Object()` create the expected objects.
- ✅ `setImmediate`/`process.nextTick` are available and queue callbacks.

## Verification

```bash
cargo test -p quench-runtime
cargo test
cargo run -- --bundle examples/simple.js
cargo run -- --bundle examples/counter.js
```
