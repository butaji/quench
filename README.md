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

## test262 Runner — 122 stages, no skips, no checkpoints

Implementation status (SSOT) is the stage test run itself:
`TEST262_STAGE=<N> TEST262_DIGEST=1 cargo test -p quench-runtime --test test262`.
If a stage is not 100% in the test runner output, it is not done.

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

Fast milestone flow:

```bash
bash tools/milestone-go.sh                  # run next stage, advance, commit
bash tools/milestone-go.sh --push           # ...and push
bash tools/milestone-go.sh --message "msg"  # custom commit message
```

`tools/milestone-go.sh` is SSOT-first: it only advances on successful test-run.

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
