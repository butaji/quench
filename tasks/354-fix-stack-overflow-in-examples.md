> **Stack overflow and example-health tracker. The proper fix is Task 85 (trampoline interpreter).**

# Task 354: Fix stack overflow in examples

## Status: IN PROGRESS

## Root cause

The JavaScript interpreter is recursive. Deep call chains in `runtime.js` exhaust the native Rust stack before the depth counter catches them.

## Current state (2026-07-09)

The examples no longer crash with a stack overflow. Task 292 (var hoisting / TDZ) is now completed, which resolved the earlier `rootId`/`props` initialization errors. The examples now all reach `Root node: Some(1)`.

A remaining `ReferenceError: Cannot access 'inst' before initialization` is still logged during `render: mountTree`, but it no longer prevents the examples from producing a valid root node. That error is separate from hoisting/TDZ and is not a stack-overflow issue.

| Example | Result |
|---|---|
| `cargo run -- examples/counter.js` | Logs `ReferenceError: Cannot access 'inst' before initialization`; returns `Root node: Some(1)` |
| `cargo run -- examples/use-bridge.tsx` | Logs `ReferenceError: Cannot access 'inst' before initialization`; returns `Root node: Some(1)` |
| `cargo run -- examples/animations.tsx` | Logs `ReferenceError: Cannot access 'inst' before initialization`; returns `Root node: Some(1)` |

## Fix options

1. **Increase Rust stack size** — quick, platform-dependent, does not scale.
2. **Trampoline / iterative interpreter** — the correct, permanent fix (Task 85).
3. **Reduce frames per JS call** — may buy a little headroom but does not solve the problem by design.

## Recommended approach

The project roadmap already mandates **Task 85** (trampoline interpreter with explicit `Vec<CallFrame>`) as Batch 1 foundation work. Do not rely on Option 1/4 as a substitute.

## Acceptance criteria

- [ ] `cargo run -- examples/counter.js` completes without error.
- [ ] `cargo run -- examples/use-bridge.tsx --prop theme=dark` completes without error.
- [ ] `cargo run -- examples/animations.tsx` completes without error.
- [ ] `cargo test -p quench-runtime` still passes (or only fails on unrelated known issues).

## Targets

- **Suite:** `runtime`
- **Batch:** 1
- **Target subset:** n/a (runtime architectural work)
- **Blocked by:** 85
- **Exit criteria:** The above examples run cleanly without stack overflow or initialization errors.

## Verification

```bash
cargo run -- examples/counter.js
cargo run -- examples/use-bridge.tsx --prop theme=dark
cargo run -- examples/animations.tsx
cargo test -p quench-runtime
```
