# Quench

JavaScript runtime targeting **100% test262 conformance**, staged to
100% per stage, with the **minimum possible LOC** as a small Rust core
plus a self-hosted JS builtins layer *(currently dormant — see
`docs/review-2026-08.md`)*. Native TS/TSX/JSX via OXC.

See `AGENTS.md` for the rules, `docs/architecture.md` for the split,
and `tasks/refactor-plan.md` for the active queue.

## Quick Start

```bash
cargo build -p quench-runtime
cargo nextest run -p quench-test262 --test test262 --profile test262 -E 'test(test262_staged)' --run-ignored all --no-capture
# Optional faster unit and harness execution (install cargo-nextest first)
cargo nextest run -p quench-runtime
cargo nextest run -p quench-test262 --test test262 --profile test262 -E 'test(test262_staged)' --run-ignored all
```

## test262 Runner — 122 stages, no skips, no checkpoints

`ssot` is the canonical shorthand for the `test-run` workflow (status and run are one flow):
`TEST262_STAGE=<N> TEST262_DIGEST=1 cargo nextest run -p quench-test262 --test test262 --profile test262 -E 'test(test262_staged)' --run-ignored all --no-capture`.
If a stage is not 100% in the test runner output, it is not done.
`test-run` is a compatibility wrapper and maps to the same SSOT flow.

Stages live in `crates/quench-test262/src/test262/runner` and
mirror `tasks/index.json`. Each stage runs to **100% passing** before
the next is touched. Only `test/intl402` (ECMA-402, separate suite) and
`test/staging` (pre-draft) are out of scope.

```bash
cargo nextest run -p quench-test262 --test test262 --profile test262 -E 'test(test262_staged)' --run-ignored all --no-capture     # current stage
TEST262_STAGE=N cargo nextest run -p quench-test262 --test test262 --profile test262 -E 'test(test262_staged)' --run-ignored all --no-capture   # specific
ALL_STAGES=1      cargo nextest run -p quench-test262 --test test262 --profile test262 -E 'test(test262_staged)' --run-ignored all --no-capture   # stop on first fail
```

On 100% the runner prints `ALL STAGES COMPLETE — Stage N: X/X`. Strict
mode: every non-`raw` test runs sloppy, then with `"use strict";`.

Fast milestone flow (`ssot`, the canonical flow):

```bash
bash tools/ssot                               # show current test-run status
bash tools/ssot --status                     # same as above
bash tools/ssot --run --fast                 # run current stage (fast)
bash tools/ssot --ship                       # run current stage fast, commit milestone
bash tools/ssot --ship --push                # run current stage fast, commit, push milestone
bash tools/milestone-run.sh --run --commit --push
                                          # fast run + commit + push current stage
bash tools/ssot --sprint                     # check blockers, then run fast current stage
bash tools/ssot --sprint --commit            # run fast + commit current milestone
bash tools/ssot --sprint --commit --push      # run fast + commit + push milestone
bash tools/ssot --run --commit               # run + commit current stage milestone
bash tools/ssot --run --commit --push         # run + commit + push current stage milestone
bash tools/ssot --next                       # print next pending stage id
```
`tools/ssot` is the canonical short path: `--next` targets next pending stage, default targets current.

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
