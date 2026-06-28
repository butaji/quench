# Goal: Drop QuickJS and run Quench on a custom minimal JS runtime

Replace the `rquickjs` dependency with an in-house interpreter that supports the JS subset used by the Quench compiler and `runtime.js`, while preserving the existing Rust bridge, Ink node tree, timer/event-loop, and parity tests.

## Approach

We are building a **custom JS execution engine**, not a custom JS parser or a from-scratch standard library. Use battle-tested crates for everything that is not the engine's core job.

- **Parsing:** use **swc** (`swc_ecma_parser` + `swc_ecma_ast`) to parse JS source, then lower to a small runtime AST. Do not write a lexer or parser by hand.
- **Engine core:** implement only what makes this runtime *custom* — the value model, scope/closure machinery, eval loop, host-function bridge, and Ink integration — under `src/js_runtime/`.
- **Support primitives:** use crates for interned strings, ordered maps, bigints/decimals, shape flags, and fast HashMaps instead of writing them.
- **Bridge:** expose existing FFI functions as host functions (`__ink_call`, `__ink_call_fast`) and Ink component tags as globals (`Box`, `Text`, etc.).
- **Runtime JS:** keep the reconciler/hooks implementation in `src/runtime.js` but rewrite unsupported JS constructs to the supported subset.
- **Swap:** replace `rquickjs::Context` with `js_runtime::Context` in `src/main.rs` and `src/event_loop.rs`, then remove `rquickjs` from dependencies.
- **Validate:** run `cargo test` and the existing example apps.

## Principles

- **Custom engine, not custom parsers.** swc handles lexing/parsing.
- **Custom engine, not custom collections.** `indexmap`, `hashbrown`, `lasso`, `string_cache` handle data structures.
- **Custom engine, not custom numeric types.** `num-bigint` and `rust_decimal` handle BigInt/decimal.
- **Custom engine, not custom shape metadata.** `bitflags` handles object shape flags.
- **No unnecessary code changes outside the engine.** The bridge, renderer, compiler, and Ink runtime stay untouched.

## Tech stack

| Crate | Purpose |
|-------|---------|
| `swc_common`, `swc_ecma_parser`, `swc_ecma_ast` | Parse JS source and inspect the AST. |
| `string_cache` / `lasso` | Interned strings for property names and identifiers. |
| `indexmap` | Ordered maps for JS object property enumeration order. |
| `num-bigint` / `rust_decimal` | `BigInt` and precise decimal arithmetic. |
| `bitflags` | Object shape flags and internal slots. |
| `hashbrown` | Fast `HashMap` for property lookups and internal tables. |

## Non-goals

- Full ECMAScript compliance.
- JIT, generational GC, or spec-compliant prototype chains beyond what `runtime.js` needs.
- Rewriting the reconciler in Rust (keep JS `runtime.js`).

## Boundaries — do not touch

These modules must stay unchanged except where the task explicitly says to call into them from the new engine:

- `src/bridge/` — node, tree, timers, I/O, props, FFI dispatch tables. Only host bindings in `src/js_runtime/host.rs` may call `crate::bridge::*`.
- `src/ink/` — the native Ink runtime and Yoga layout code.
- `src/render/` — text measurement, keycode mapping, color, render tree.
- `src/compiler/` — the TSX/TS compiler and its JS output format.
- `src/cli.rs`, `src/signals.rs`, `src/hotreload.rs` — CLI, signal, and hot-reload plumbing.

Allowed glue points:
- `src/js_runtime/` — new custom engine.
- `src/main.rs` — runtime initialization, loading `runtime.js`, and user code.
- `src/event_loop.rs` — only the JS context type and the calls into JS dispatch functions.
- `src/ink_js.rs` — only rquickjs-specific code may be removed/replaced with interpreter registration.
- `Cargo.toml`, `build.rs` — only remove the `rquickjs` dependency and QuickJS bytecode references.

## Task index

See `tasks/index.json`.

## Verification

```bash
cargo check
cargo test
cargo run -- examples/simple.js
cargo run -- examples/counter.js
```

## Execution options

1. **Subagent-driven**: dispatch one subagent per `tasks/*.md` and review between tasks.
2. **Inline execution**: work through the tasks in this session with checkpoints after each file compiles.
