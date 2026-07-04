> **North star.** 100% compatibility with JavaScript, TypeScript, TSX, and JSX, executing natively, with minimum code and maximum performance. No bytecode, no JIT.

# Task 296: Reach 100% JS/TS/TSX/JSX compatibility

## Status: IN PROGRESS

## Goal

Make Quench pass the full test262 and TypeScript conformance suites for all runtime semantics, while keeping the implementation small and the hot path fast.

## Constraints

- No bytecode VM.
- No JIT.
- Native execution of `.ts/.tsx/.js/.jsx` via `swc` parsing and lowering.
- Minimum code surface; maximum hot-path performance.

## Current baseline

| Suite | Pass rate (total) | Pass rate (non-skipped) |
|---|---|---|
| test262 subset | 16.9% | 54.9% |
| TypeScript expressions | 45.0% | 45.0% |
| TypeScript full | 14.0% | 22.9% |

## Strategy

1. **Unblock measurement** (Tasks 253, 91, 250, 97).
2. **Quick syntax/builtin wins** (Tasks 281, 291, 290, 289, 283, 282, 284, 286).
3. **Core semantics** (Tasks 292, 293, 141, 295, 294).
4. **Big architecture** (Tasks 85, 241, 182, 251, 88, 264).
5. **Full conformance** (Task 82).

## Acceptance criteria

- [ ] test262 language + built-ins core suites pass at 100%.
- [ ] TypeScript runtime conformance passes at 100%.
- [ ] All examples run natively without transpilation.
- [ ] Hot-path benchmarks improve with each architecture milestone.

## Verification

```bash
cargo test -p quench-runtime --test test262 -- --ignored
cargo test -p quench-runtime --test conformance -- --test-threads=1
timeout 60 cargo run -- examples/counter.js
timeout 60 cargo run -- examples/use-bridge.tsx --prop theme=dark
```
