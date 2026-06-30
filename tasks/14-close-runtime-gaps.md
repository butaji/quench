# Task 14: Close remaining runtime gaps

## Goal

Fix the specific interpreter and built-in gaps that block real Ink examples from running end-to-end.

> This task collects the smaller items that remain after the major parser/lowering/built-in/prototype work in Tasks 01–04.

## Pareto & reuse note

- Prefer existing crates, the Rust standard library, and OS features over custom code.
- Follow the 80/20 rule: implement the subset that unblocks the targeted examples/conformance tests first.
- Defer edge cases, but document them in this task or spawn a follow-up task so they are not lost.

## Files

- `crates/quench-runtime/src/lower/expr.rs`
- `crates/quench-runtime/src/lower/decl.rs`
- `crates/quench-runtime/src/interpreter/call.rs`
- `crates/quench-runtime/src/interpreter/eval_expr/*.rs`
- `crates/quench-runtime/src/builtins/array.rs`
- `crates/quench-runtime/src/builtins/promise.rs`
- `crates/quench-runtime/src/builtins/map.rs`
- `crates/quench-runtime/src/value/mod.rs`
- `src/event_loop.rs`

## Remaining gaps

1. **Optional chaining lowering** (`obj?.prop`, `obj?.[expr]`, `obj?.()`)
   - `lower/expr.rs` currently returns an error for `OptChain`.
   - Lower it to a conditional expression that checks `obj != null && obj != undefined`.

2. **Destructuring assignment**
   - `lower/expr.rs` rejects assignment patterns (`[a,b] = arr`, `({x} = obj)`).
   - Implement lowering and interpreter assignment for array/object patterns.

3. **Destructuring function/arrow parameters**
   - `lower/decl.rs` maps non-identifier params to `"arg"` and drops patterns.
   - Lower destructuring params to local destructuring assignments in the function body.

4. **Rest parameters in arrow functions**
   - Arrow functions ignore rest params in the lowerer.
   - Apply the same rest-param handling used for function declarations.

5. **`arguments` object in JS-to-JS calls**
   - `interpreter/call.rs` does not inject an `arguments` local for ordinary JS-to-JS function calls.
   - Bind `arguments` to an array-like object containing the actual arguments.

6. **`Promise` static methods on the constructor**
   - `Promise.resolve`, `Promise.all`, `Promise.race` are currently installed on `Promise.prototype`.
   - Move them to the `Promise` constructor object.

7. **`Array.from` iterable support**
   - `Array.from` only copies `.elements`.
   - Add a branch that iterates `Map`/`Set`/array-like objects and collects their values.

8. **Callable `Array`/`Object` constructors**
   - `new Array()` and `new Object()` fail because the constructor objects are not callable.
   - Add `__call`/`constructor` wiring so they can be used with `new`.

9. **Event-loop microtask invocation**
   - `src/event_loop.rs` never calls `__tb_invoke_microtasks`.
   - `__ink_enqueue_microtask` in the bridge also discards the callback string.
   - Wire both so `setImmediate`/`process.nextTick` callbacks actually run.

10. **Map insertion-order iteration**
    - `for...of` over `Map` currently iterates `HashMap` keys.
    - If observable order matters, switch internal storage to an ordered map or maintain insertion-order key list.

## Boundaries

- Only modify `crates/quench-runtime/src/` and `src/event_loop.rs`.
- Do not touch `src/bridge/` internals beyond the existing `__ink_enqueue_microtask` host call.
- `examples/` are immutable; fix the runtime to make them pass.

## Acceptance criteria

- `config.platform?.os` evaluates safely (returns `undefined` if `platform` is missing).
- `Object.entries(config).map(([k, v]) => ...)` binds `k` and `v` correctly.
- `runtime.js` console polyfill and `createElement` can read `arguments`.
- `Promise.resolve(42).then(v => v)` resolves to `42`.
- `Array.from(new Set([1,2]))` returns `[1,2]`.
- `new Array(1,2,3)` and `new Object()` create the expected objects.
- `setImmediate`/`process.nextTick` callbacks are drained during the event loop.

## Verification

```bash
cargo test -p quench-runtime
cargo run -- examples/use-bridge.tsx --prop theme=dark --prop user=admin
cargo run -- examples/counter.js
```
