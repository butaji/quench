> **Stack overflow and example-health tracker. The proper fix is Task 85 (trampoline interpreter).**

# Task 354: Fix stack overflow in examples

## Status: IN PROGRESS

## Root cause

The JavaScript interpreter is recursive. Deep call chains in `runtime.js` exhaust the native Rust stack before the depth counter catches them.

## Current state (2026-07-08)

The examples no longer crash with a stack overflow, but they hit `ReferenceError: Cannot access '...' before initialization` because hoisting/TDZ is not fully correct. That work is tracked in Task 292.

| Example | Result |
|---|---|
| `cargo run -- examples/counter.js` | Logs `ReferenceError: Cannot access 'rootId' before initialization`; returns `Root node: Some(1)` |
| `cargo run -- examples/use-bridge.tsx --prop theme=dark` | `ReferenceError: Cannot access 'props' before initialization`; `Root node: None` |
| `cargo run -- examples/animations.tsx` | `ReferenceError: Cannot access 'props' before initialization`; `Root node: None` |

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
