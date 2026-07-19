# AGENTS.md

Guidance for agentic work in this repository.

## What this is

Quench — a Rust-native JavaScript runtime (OXC parser + custom tree-walking
interpreter) targeting 100% test262 conformance. Single crate:
`crates/quench-runtime`. The test262 suite lives in the `tests/test262`
submodule; never modify files inside it.

## Commands

```bash
cargo build -p quench-runtime
cargo test -p quench-runtime                     # lib + integration + harness smoke tests
cargo fmt -p quench-runtime                      # CI gate: fmt --check
cargo clippy -p quench-runtime --all-targets     # CI gate: zero warnings (-D warnings)

# Staged test262 runner (fail-fast, checkpointed)
cargo test -p quench-runtime --test test262 test262_staged -- --ignored --nocapture
rm crates/quench-runtime/.test262_checkpoint     # restart from stage 0
TEST262_LIMIT=10 cargo test -p quench-runtime --test test262 test262_staged -- --ignored --nocapture
TEST262_STAGE=0  cargo test -p quench-runtime --test test262 test262_staged -- --ignored --nocapture
```

`TEST262_LIMIT` batches a run while still saving the checkpoint;
`TEST262_STAGE` runs one stage without touching the checkpoint (CI mode).

## Staged-runner workflow (test by test)

1. Run the staged runner. It stops at the first failure and prints the test
   path, stage, and index.
2. Fix the engine (minimal change — see conventions below).
3. Rerun. The checkpoint at `crates/quench-runtime/.test262_checkpoint`
   auto-resumes where you stopped.
4. Leave the checkpoint at the first genuine language-feature failure; do not
   skip tests just to advance it.

Every non-`raw` test runs twice: sloppy and with `"use strict";` prepended.
A `strict mode:` prefix on the failure means only the strict run failed.

## Architecture map

```
crates/quench-runtime/src/
├── parser.rs   # OXC JS/TS/TSX/JSX parsing → internal AST
├── lower/         # AST lowering (tagged templates, switch→if-chains, etc.)
├── ast.rs         # internal AST
├── interpreter.rs # entry points, strict-mode flag, native-this, hoisting
├── eval/          # tree-walking evaluator (expression, statement, call,
│                  #   object, member/, function, class, operators, ...)
├── env.rs         # lexical environments / scopes
├── value/         # Value model: Object, ValueFunction, NativeFunction,
│                  #   NativeConstructor, error creation
├── builtins/      # Rust-native builtins (object, array, string, number,
│                  #   map, symbol, promise, regex, array_buffer, ...)
├── context/       # Context: globals + CURRENT_CONTEXT thread-local
└── test262/       # runner.rs (staged/checkpoint), harness/ (native
                   #   injection + JS harness loading), skip.rs, metadata.rs
```

Pipeline: source → `parser` → `lower/` → `interpreter::eval` → `eval/`.

## Conventions

- **Builtins throw `JsError`, never panic.** Use
  `crate::value::error::create_js_error_with_type` and
  `crate::value::set_thrown_value` so `try/catch` and `assert.throws` see a
  proper JS error object.
- **Minimal diffs.** Match the surrounding file's style; no opportunistic
  refactors, no new dependencies without a strong reason.
- **Skip policy lives in `src/test262/skip.rs`** — the single source of truth:
  - `SKIP_FEATURES` / `SKIP_FLAGS`: frontmatter-driven skips.
  - `should_skip_source`: source-level skip (currently async syntax:
    `async function`, `async(`, `await `).
  - `SKIP_TEST_PATHS`: individual tests incompatible with the runner's
    harness model (keep this list as short as possible).
- **`TOLERATED_EVAL_FAILURES`** in `src/test262/harness/mod.rs`: harness JS
  files allowed to fail at load time (currently only
  `resizableArrayBufferUtils.js`, pending Uint8Array). Every other harness
  file MUST load cleanly.
- **Symbols**: a `Value::Symbol` payload is the raw `desc\0id` string. The
  payload is used directly as the property key everywhere (computed member
  access, `Object.defineProperty`, `hasOwnProperty`, `delete`). Use
  `to_property_key`-style conversion (symbol → payload, else `to_js_string`);
  never `to_js_string` a symbol into a property key.
- **Boxed primitives** (`Object("a")`, `new Number(1)`, ...): payload stored
  via `builtins::object::set_boxed_value` as a non-enumerable `_value`
  property (stands in for [[PrimitiveData]]); `boxed_object(name)` links the
  wrapper to the constructor's prototype so `instanceof` and `valueOf` work.
- **Function strictness is captured at definition** (`ValueFunction.strict`),
  never inherited from the call site. Class bodies set `strict = true`
  unconditionally. Functions created by the `Function` constructor stay
  sloppy unless their body has a `"use strict"` directive.
- **Accessor properties**: `Object.defineProperty` with `get`/`set` stores
  the function values via `Object::define_accessor` (AST getters/setters from
  object literals use `set_getter`/`set_setter`). `GetterStorage.func` takes
  precedence at call time and preserves identity for descriptors.
- **`CURRENT_CONTEXT`** (context/mod.rs): `thread_local` raw pointer set for
  the duration of eval; deref only after the `Option` is `Some`, and only
  from native code that runs during eval.
- New builtins are wired in `builtins/mod.rs::register_builtins` — mind the
  documented ordering constraints (Symbol before Map/Set, Number before Date).
- Dependency choices (why a crate is in `Cargo.toml`, what it replaces) live
  in `crates/quench-runtime/DEPENDENCIES.md`. Update that file in the same
  diff when adding or removing a direct dependency.
