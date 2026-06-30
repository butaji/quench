# Task 19: Pass TypeScript iterator, module, and async conformance tests

## Goal

Make the runtime pass all runtime-relevant iterator, module, and async conformance cases.

## Files

- `crates/quench-runtime/src/lower/stmt.rs`
- `crates/quench-runtime/src/lower/expr.rs`
- `crates/quench-runtime/src/interpreter/eval_stmt/loops.rs`
- `crates/quench-runtime/src/builtins/promise.rs`
- `crates/quench-runtime/src/builtins/array.rs`
- `crates/quench-runtime/src/builtins/map.rs`
- `crates/quench-runtime/src/context/mod.rs`
- `src/event_loop.rs`

## Steps

1. From the Task 16 audit, pick the iterator/module/async failures.
2. Implement or fix the missing features, which are likely to include:
   - `Symbol.iterator` support or special-case iterable handling for `Map`/`Set`/arrays
   - `Array.from` consuming iterables
   - generator functions and `yield` (if any conformance cases require them)
   - `import`/`export` execution in modules (the runtime already parses modules; wire module loading)
   - `Promise` static methods on the constructor object (`resolve`, `all`, `race`, `reject`)
   - `async`/`await` desugaring or native support
   - event-loop microtask invocation (`__tb_invoke_microtasks`) so `setImmediate`/`process.nextTick`/Promise microtasks run
3. Add unit tests for each fixed feature.
4. Re-run the iterator/module/async conformance subset.

## Boundaries

- Only modify `crates/quench-runtime/src/` and `src/event_loop.rs` for microtask invocation.
- Do not touch `src/bridge/`, `src/ink/`, `src/render/`, `src/compiler/` beyond the existing host-call interface.
- Do not modify `tests/typescript/` or `examples/`.

## Acceptance criteria

- `for (const x of new Set([1,2,3]))` works.
- `Array.from(new Map([['a',1]]))` returns `[['a',1]]`.
- `Promise.resolve(42).then(v => v)` resolves.
- `async function f() { return 1; } f().then(console.log)` works.
- Module-level `import`/`export` code executes correctly.

## Verification

```bash
cargo test -p quench-runtime --test conformance -- iterators modules async
```
