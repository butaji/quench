# Task 24: Reactive execution engine over the HIR

## Goal

Implement the runtime side of the reactive HIR: build a dependency graph from `Signal`, `Memo`, `Effect`, and `Render` nodes and propagate changes efficiently.

## TDD & testing note

- Follow the red-green-refactor cycle: write a failing unit test first, then the minimal code to pass it, then refactor.
- Add a regression test for every bug fix and edge case covered by this task.
- Keep tests in `crates/quench-runtime/tests/` and run `cargo test -p quench-runtime` before marking work done.


## Files

- `crates/quench-runtime/src/interpreter/mod.rs`
- `crates/quench-runtime/src/interpreter/reactive.rs` (new)
- `crates/quench-runtime/src/value/mod.rs`
- `crates/quench-runtime/src/context/mod.rs`
- `src/event_loop.rs`

## Background

The HIR describes reactive primitives explicitly. This task makes the interpreter actually use them:

- A `Signal` is a mutable cell.
- A `Memo` caches a pure computation and re-runs only when its dependencies change.
- An `Effect` runs a side-effect callback after the reactive graph stabilizes.
- A `Render` node represents a component boundary and re-renders only when its props or used signals change.

## Steps

1. Create `crates/quench-runtime/src/interpreter/reactive.rs` with:
   - `ReactiveGraph` — owns all signals, memos, effects, and render nodes.
   - `SignalId`, `MemoId`, `EffectId`, `RenderId` handles.
   - Dependency tracking: when a `Memo`/`Effect`/`Render` evaluates, record every `SignalGet` it performs.
2. Implement signal write:
   - `SignalSet` marks the signal as changed.
   - Walk the dependent memos/effects/renders and mark them dirty.
   - Re-evaluate dirty memos depth-first; if a memo's value changes, propagate to its dependents.
3. Implement effect scheduling:
   - Collect dirty effects during propagation.
   - Run them in a single batch after the graph stabilizes.
4. Implement render scheduling:
   - Collect dirty `Render` nodes.
   - Re-run their component function to produce a new element tree.
   - Feed the new tree to the existing Ink reconciler via the bridge.
5. Wire into `src/event_loop.rs` so timer/keyboard/mouse events can trigger signal writes and the engine flushes the reactive graph before the next render.

## Boundaries

- Only modify `crates/quench-runtime/src/` and `src/event_loop.rs`.
- Do not touch `src/bridge/` internals beyond existing host calls.
- Do not modify `examples/` or `tests/typescript/`.

## Pareto & reuse note

- Start with a simple push-based reactive graph; do not build a full signal-graph library from scratch unless needed.
- Reuse the existing Ink reconciler in `src/bridge/` for actual DOM/terminal updates.
- Defer advanced features (effect cleanup functions, suspense, concurrent rendering) until basic reactivity works end-to-end.

## Acceptance criteria

- `const [count, setCount] = useState(0); setCount(count + 1)` re-renders the component with the new value.
- A `useMemo` only re-computes when its dependency changes.
- A `useEffect` runs after the render commit and re-runs when its dependency changes.
- Multiple signal writes in one event tick batch into a single render pass.

## Verification

```bash
cargo test -p quench-runtime
cargo run -- examples/counter.js
```
