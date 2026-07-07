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

| Suite | Full spec size | Current subset | Pass rate (of subset) | True spec coverage |
|---|---|---|---|---|
| test262 | ~53,683 files | 431 | 10.9% | 0.09% |
| TypeScript expressions | ~18,876 cases | 376 | 40.7% | 0.81% |

## Strategy

1. **Unblock measurement** (Tasks 253, 91, 250, 97).
2. **Quick syntax/builtin wins** (Tasks 281, 291, 290, 289, 283, 282, 284, 286).
3. **Core semantics** (Tasks 292, 293, 141, 295, 294).
4. **Big architecture** (Tasks 85, 241, 182, 251, 88, 264).
5. **Full conformance** (Task 82).

## Acceptance criteria

- [ ] 100% of the full test262 suite (~53,683 files) passes.
- [ ] 100% of the full TypeScript runtime conformance suite (~18,876 cases) passes.
- [ ] All examples run natively without transpilation.
- [ ] Hot-path benchmarks improve with each architecture milestone.

## Verification

```bash
cargo test -p quench-runtime --test test262 -- --ignored
cargo test -p quench-runtime --test conformance -- --test-threads=1
timeout 60 cargo run -- examples/counter.js
timeout 60 cargo run -- examples/use-bridge.tsx --prop theme=dark
```

## Targets

- **Suite:** `both`
- **Batch:** 7
- **Target subset:** Full `tests/test262` + `tests/typescript` conformance suites
- **Blocked by:** 82
- **Exit criteria:** 100% of full test262 and TypeScript runtime conformance suites pass with zero spec skips.
