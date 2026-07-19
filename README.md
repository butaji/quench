# Quench

JavaScript runtime targeting **100% test262 conformance**, staged to
100% per stage, with the **minimum possible LOC** as a small Rust core
plus a self-hosted JS builtins layer. Native TS/TSX/JSX via OXC.

See `AGENTS.md` for the rules, `docs/architecture.md` for the split,
and `tasks/refactor-plan.md` for the active queue.

## Quick Start

```bash
cargo build -p quench-runtime
cargo test -p quench-runtime --test test262 test262_staged -- --ignored --nocapture
```

## test262 Runner — 93 stages, no skips, no checkpoints

Stages live in `crates/quench-runtime/src/test262/runner.rs::STAGES` and
mirror `tasks/index.json`. Each stage runs to **100% passing** before
the next is touched. Only `test/intl402` (ECMA-402, separate suite) and
`test/staging` (pre-draft) are out of scope.

```bash
cargo test -p quench-runtime --test test262 test262_staged -- --ignored --nocapture     # current stage
TEST262_STAGE=N cargo test -p quench-runtime --test test262 test262_staged -- --ignored --nocapture   # specific
ALL_STAGES=1      cargo test -p quench-runtime --test test262 test262_staged -- --ignored --nocapture   # stop on first fail
```

On 100% the runner prints `ALL STAGES COMPLETE — Stage N: X/X`. Strict
mode: every non-`raw` test runs sloppy, then with `"use strict";`.

## TypeScript / JSX

```rust
let mut ctx = Context::new()?;
ctx.eval_typescript(include_str!("src/main.ts"))?;
```

See `crates/quench-runtime/tests/native_extensions.rs`.

## CI

fmt → clippy → build → integration tests → test262 stages in parallel.

## License

MIT