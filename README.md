# Quench

Rust-native JavaScript runtime targeting **100% test262 ECMAScript conformance**.

Native TypeScript/TSX/JSX support via swc parsing + custom interpreter.

## Quick Start

```bash
# Build
cargo build -p quench-runtime

# Run staged test262 conformance suite
cargo test -p quench-runtime --test test262 test262_staged -- --ignored --nocapture

# Resume from checkpoint after fixing a test
# Just rerun the same command — the runner auto-resumes from
# crates/quench-runtime/.test262_checkpoint (stage,index)
cargo test -p quench-runtime --test test262 test262_staged -- --ignored --nocapture

# Force restart from stage 0
rm crates/quench-runtime/.test262_checkpoint && cargo test -p quench-runtime --test test262 test262_staged -- --ignored --nocapture
```

## Architecture

```
crates/quench-runtime/src/
├── test262/      # Staged runner + checkpoint logic
├── swc_parse.rs  # swc-based JS/TS/TSX/JSX parser
├── interpreter.rs # AST interpreter
├── builtins/     # Rust-native built-in implementations
├── context/      # Runtime execution context
├── eval/         # Statement/expression evaluation
├── lower/        # AST lowering passes
└── value/       # JS value model
```

## TypeScript / JSX Support

Native TS/TSX/JSX support via swc. Parse and execute directly:

```rust
use quench_runtime::Context;

let mut ctx = Context::new()?;
// Parse and run TS/TSX/JSX directly
ctx.eval_typescript(include_str!("src/main.ts"))?;
```

See `crates/quench-runtime/tests/native_extensions.rs` for examples.

## test262 Staged Runner

- **51 ordered stages** — language/literals → built-ins → modules → annexB
- **Auto-checkpoint** — `.test262_checkpoint` saves `stage,index`
- **Fail-fast** — stops at first failure, resume with one command
- **Feature gates** — skips unimplemented features silently
- **No YAML crate** — frontmatter parsed inline
- **CI parallel** — all 51 stages run in parallel in CI

### Workflow

1. Run staged runner — first failure prints file + stage + index
2. Fix the engine
3. Recompile, rerun — checkpoint auto-advances
4. Repeat until all stages pass

### Runner environment variables

- `TEST262_LIMIT=N` — run at most N tests this invocation; the checkpoint
  is saved at the resume position, so repeated runs advance in batches.
- `TEST262_STAGE=N` — run only stage N (used by CI); the checkpoint file
  is never read, written, or deleted in this mode.
- `TEST262_DIR=/path/to/test262` — override the test262 checkout location
  (defaults to `tests/test262`).

### Strict mode

Every non-`raw` test runs twice: sloppy, then again with `"use strict";`
prepended. `raw`/`noStrict` tests run sloppy only, `onlyStrict` strict only.
A strict-mode failure is reported with a `strict mode:` prefix. Both runs
must satisfy the test's `negative:` frontmatter expectations.

## CI

Single workflow (`.github/workflows/ci.yml`): fmt → clippy → build → integration tests → 51 test262 stages in parallel → summary.

## License

MIT
