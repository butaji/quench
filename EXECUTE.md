> **Execution contract for the Quench runtime.**

# Goal

Finish the custom TS/JS/TSX runtime in `crates/quench-runtime/` and keep it Ink-compatible.

## Principles

1. **Reuse before rewriting.** Prefer crates and stdlib. Mandatory crates: `swc`, `serde_json`, `regress`, `miette`/`ariadne`, `lasso`, `indexmap`, `num-bigint`/`rust_decimal`, `bumpalo`, `rustc-hash`/`foldhash`, `thiserror`.
2. **80/20 Pareto.** Unblock examples and conformance tests first.
3. **No cross-compilation.** Execute `.ts/.js/.tsx/.jsx` natively via swc transforms.
4. **Strict build-time linting.** Max 500 lines/file, 40 lines/function, complexity 10. No `#[allow(...)]` exemptions.
5. **Test-driven development.** Every bug fix and feature starts with a failing unit test.
6. **Document deferrals.** Postponed features must be tracked in `tasks/index.json`.

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
