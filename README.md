# Quench

Rust-native JavaScript runtime targeting **100% test262 ECMAScript conformance**.
Native TypeScript/TSX/JSX via OXC parsing + custom interpreter.

## Quick Start

```bash
cargo build -p quench-runtime
cargo test -p quench-runtime --test test262 test262_staged -- --ignored --nocapture
```

## Architecture

```
crates/quench-runtime/src/
├── test262/     # Staged runner (no checkpoints, no skips)
├── parser.rs    # OXC-based JS/TS/TSX/JSX parser
├── interpreter.rs
├── builtins/    # Rust-native built-in implementations
├── context/     # Runtime execution context
├── eval/        # Statement/expression evaluation
├── lower/       # AST lowering passes
└── value/      # JS value model
```

## test262 Runner

**51 ordered stages** — literals → identifiers → expressions → statements → built-ins → modules → annexB.

No checkpoints. No skips. Each stage runs to 100% passing, then move to the next.

```bash
# Run current stage (stage 0)
cargo test -p quench-runtime --test test262 test262_staged -- --ignored --nocapture

# Run specific stage
TEST262_STAGE=1 cargo test -p quench-runtime --test test262 test262_staged -- --ignored --nocapture
```

Strict mode: every non-`raw` test runs sloppy, then with `"use strict";`. `raw`/`noStrict` run sloppy only, `onlyStrict` strict only.

## Workflow: unit tests, not guesswork — enforced, no exceptions

**You do not debug. You do not guess. You write a failing unit test first.
Every. Single. Time. No exceptions.**

A failing test262 case, a bug, a new builtin, a parser or evaluator change —
all enter the codebase through the same gate: a `#[test]` that asserts the
exact behavior, committed *before* any production change. If you cannot
express the behavior as a unit test, you do not understand it yet, and you are
not allowed to touch production code.

### Forbidden

- `println!` / `dbg!` archaeology. **Never.**
- Reading code until it "looks wrong" and patching. **Never.**
- "Let me try this" speculative edits. **Never.**
- Refactors done "while I'm here" without a test. **Never.**
- Skipping the failing-test step "just this once". **Never.**
- Editing `tests/test262.rs` or anything under `tests/test262/`. **Never.**

### Mandatory cycle, in order

1. **Reproduce** — add a `#[test]` in the module's `mod tests` (or
   `crates/quench-runtime/tests/`) that exercises the exact JS/Rust behavior.
   Mirror the style in `src/eval/string_methods.rs` and `src/builtins/map.rs`.
2. **Watch it fail** — `cargo test -p quench-runtime <name>` must fail with the
   same symptom as the test262 case. If it doesn't fail, or fails for the wrong
   reason, delete the test and write a better one. Do not proceed.
3. **Fix** — minimal change to production code so the unit test passes. No
   opportunistic refactors.
4. **Verify** — re-run the unit test, the module's whole test suite, then the
   relevant test262 stage. `cargo fmt` / `cargo clippy --all-targets` until
   both are clean. Linter warnings block the fix from being "done".
5. **Leave the test in** — regression coverage stays in the tree. A fix
   without a committed test is not done.

test262 output signals *what* to test; the reproduction lives as a unit test
next to the code — the conformance run in `tests/test262.rs` is never edited.

## TypeScript / JSX

```rust
let mut ctx = Context::new()?;
ctx.eval_typescript(include_str!("src/main.ts"))?;
```

See `crates/quench-runtime/tests/native_extensions.rs` for examples.

## CI

fmt → clippy → build → integration tests → test262 stages in parallel.

## License

MIT
