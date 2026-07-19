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

## Workflow: unit tests, not guesswork

**Never debug by inspection. Never patch on a hunch. Always write a failing test first.**

Every fix or new behavior goes through the same cycle, in order:

1. **Reproduce** — add a `#[test]` in the module's `mod tests` (or
   `crates/quench-runtime/tests/`) that exercises the exact JS/Rust behavior.
   Mirror the style in `src/eval/string_methods.rs` and `src/builtins/map.rs`.
2. **Watch it fail** — `cargo test -p quench-runtime <name>` must fail with the
   same symptom as the test262 case. If it doesn't, refine the test before
   touching production code.
3. **Fix** — minimal change to production code so the unit test passes.
4. **Verify** — re-run the unit test, the module's whole test suite, then the
   relevant test262 stage. `cargo fmt`/`cargo clippy` before declaring done.
5. **Leave the test in** — regression coverage stays in the tree. A fix
   without a committed test is not done.

No `println!`-driven archaeology. No speculative rewrites. No "let me try
this" patches. test262 output signals *what* to test; the reproduction lives
as a unit test next to the code — the conformance run in `tests/test262.rs` is
never edited.

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
