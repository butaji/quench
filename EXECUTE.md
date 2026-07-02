> **Execution contract for the Quench runtime.**

# Goal

Finish the custom TS/JS/TSX runtime in `crates/quench-runtime/` and keep it Ink-compatible.

## Principles

1. **High impact, low effort first.** Every decision is filtered by effort vs. payoff. Prefer the change that fixes the most failures, unblocks the most examples, or removes the biggest stability risk with the smallest patch.
2. **Reuse before rewriting.** Prefer crates and stdlib. Mandatory crates: `swc`, `serde_json`, `regress`, `miette`/`ariadne`, `lasso`, `indexmap`, `num-bigint`/`rust_decimal`, `bumpalo`, `rustc-hash`/`foldhash`, `thiserror`.
3. **80/20 Pareto.** Unblock examples and conformance tests first.
4. **No cross-compilation.** Execute `.ts/.js/.tsx/.jsx` natively via swc transforms.
5. **Strict build-time linting.** Max 500 lines/file, 40 lines/function, complexity 10. No `#[allow(...)]` exemptions.
6. **Test-driven development.** Every bug fix and feature starts with a failing unit test.
7. **Document deferrals.** Postponed features must be tracked in `tasks/index.json`.

## Decision filter

Before starting any task, rank it:

| Priority | Action |
|----------|--------|
| High impact + low effort | Do immediately. |
| High impact + high effort | Plan and split into smaller low-effort steps. |
| Low impact + low effort | Batch or defer. |
| Low impact + high effort | Do not do. |

## Boundaries

Do not touch:

- `src/bridge/`, `src/ink/`, `src/render/`
- `examples/`, `tests/test262/`, `tests/typescript/`

Allowed:

- `crates/quench-runtime/src/`
- `src/main.rs` for host-function registration
- `src/event_loop.rs` for JS dispatch
- `src/runtime.js` for compatibility shims

## Verification

```bash
cargo check
timeout 120 cargo test -p quench-runtime
timeout 60 cargo run -- examples/counter.js
timeout 60 cargo run -- examples/use-bridge.tsx --prop theme=dark
timeout 60 cargo run -- examples/animations.tsx
```

## Conformance

See `docs/conformance.md` for running the test262 and TypeScript harnesses.

## Tasks

Current work is tracked in `tasks/index.json`.
