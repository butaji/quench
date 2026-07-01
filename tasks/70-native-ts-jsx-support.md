# Task 70: Native support for `.ts/.js/.tsx/.jsx` and React optimizations

## Goal

`quench-runtime` must parse and execute `.ts`, `.js`, `.tsx`, and `.jsx` source directly, without external cross-compilation. The main `quench` binary must stop using `esbuild`/`npx`. JSX must be transformed inside the runtime with `ink.createElement`/`ink.Fragment` and React-aware optimizations.

## Design

See `docs/superpowers/specs/2026-06-28-native-ts-jsx-support-design.md`.

## Implementation plan

See `docs/superpowers/plans/2026-06-28-native-ts-jsx-support-plan.md`.

## Key requirements

- No `esbuild`/`npx` cross-compilation.
- Parser selects `Syntax::Es`/`Typescript` and `parse_script`/`parse_module` by extension and content.
- TypeScript is stripped with `swc_ecma_transforms_typescript::strip`.
- JSX is transformed with `swc_ecma_transforms_react` using `ink.createElement` factory and `ink.Fragment` fragment.
- Static children and key hoisting optimizations where possible.
- `Context::eval_file(path)` dispatches by extension.
- Regression tests for native JS/TS/JSX/TSX evaluation.

## Files to create/modify

- `crates/quench-runtime/Cargo.toml` — transform deps.
- `crates/quench-runtime/src/swc_parse.rs` — parser mode selection, transforms.
- `crates/quench-runtime/src/lib.rs` — `eval_file`, `eval_js`, `eval_ts`, `eval_jsx`, `eval_tsx`.
- `crates/quench-runtime/src/lower.rs` — remove JSX rejection arms.
- `crates/quench-runtime/src/ast.rs` — add module HIR.
- `crates/quench-runtime/src/interpreter.rs` — execute modules as scripts initially.
- `crates/quench-runtime/tests/native_parse.rs` — parser regression tests.
- `crates/quench-runtime/tests/native_extensions.rs` — end-to-end regression tests.
- `src/main.rs`, `src/cli.rs` — use `ctx.eval_file` directly.
- `src/compiler/mod.rs` — remove esbuild invocation.
- `src/runtime.js` — expose `ink.jsx`/`ink.jsxs` fast paths if needed.
- `docs/architecture.md`, `EXECUTE.md` — update docs.
- `tasks/index.json` — track this task.

## Verification

```bash
cargo test -p quench-runtime
timeout 60 cargo run -- examples/counter.js
timeout 60 cargo run -- examples/counter.tsx
timeout 60 cargo run -- examples/use-bridge.tsx --prop theme=dark
timeout 60 cargo run -- examples/animations.tsx
```

All commands must run with timeouts (see Task 31).
