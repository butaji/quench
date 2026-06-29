# Task 07: runtime.js compatibility tweaks

## Goal

Keep `runtime.js` running on the custom interpreter. Prefer adding features to the engine, but document any unavoidable runtime.js rewrites.

## Files

- `src/runtime.js`

## Current potential issues

- `??` (nullish coalescing) is used in mouse hit-testing: `depthMap.get(state.nodeId) ?? -1`. If Task 01/03 implement `??`, no rewrite is needed.
- `for...of` over `Map` and `Set` is used heavily. If Task 01/03 implement it, no rewrite is needed.
- `for...in` over props is used. If Task 01/03 implement it, no rewrite is needed.
- `arguments` object is used by `Fragment` and the console polyfill. If Task 03 implements it, no rewrite is needed.
- `get rows()` / `get columns()` on `process.stdout`. If Task 03 implements getters, no rewrite is needed.
- `Array.prototype.slice.call(arguments)` — requires both `arguments` and `Array.prototype.slice`. If Task 02/03 implement these, no rewrite is needed.
- `Object.prototype.hasOwnProperty.call(options, k)` — requires `Object.prototype`. If Task 02 implements it, no rewrite is needed.
- `String.prototype.padStart` — required by the date locale patch. If Task 02 implements it, no rewrite is needed.

## Steps

1. After completing Tasks 01–06, attempt to load `runtime.js` unchanged.
2. If any construct still fails and is cheaper to rewrite in JS than to implement in Rust, make the minimal rewrite and add a comment explaining why.
3. Do not rewrite core reconciler/hooks logic — only small polyfill-compatible shims.

## Boundaries

- Only modify `src/runtime.js`.
- Do not touch `crates/quench-runtime/src/` in this task.
- `examples/` are immutable. Any runtime.js change must preserve the original example semantics; do not rewrite examples to work around runtime bugs.

## Acceptance criteria

- `runtime.js` loads and evaluates without errors in `cargo run -- examples/simple.js`.
- Every rewrite in `runtime.js` has a comment linking to the missing engine feature.

## Verification

```bash
cargo run -- examples/simple.js
```
