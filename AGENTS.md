# AGENTS.md

Quench — Rust-native JS runtime targeting **100% test262 conformance**.
Single crate: `crates/quench-runtime`. Never modify `tests/test262`.

## Commands

```bash
cargo build -p quench-runtime
cargo test -p quench-runtime
cargo fmt -p quench-runtime
cargo clippy -p quench-runtime --all-targets

# Run one stage
cargo test -p quench-runtime --test test262 test262_staged -- --ignored --nocapture
# Specific stage
TEST262_STAGE=1 cargo test -p quench-runtime --test test262 test262_staged -- --ignored --nocapture
```

No checkpoints. No skips. Each stage runs to 100% passing before advancing.

## Workflow: unit tests, not guesswork

**Never debug by inspection. Never patch on a hunch. Always write a failing test first.**

When a test262 case fails, or when implementing/fixing a builtin, evaluator, or
parser path, the cycle is mandatory:

1. **Reproduce** — add a `#[test]` in the relevant module's `mod tests` (or in
   `crates/quench-runtime/tests/`) that exercises the exact JS or Rust behavior
   under inspection. Mirror the surrounding test style (see
   `src/eval/string_methods.rs`, `src/builtins/map.rs` for the established
   pattern).
2. **Watch it fail** — `cargo test -p quench-runtime <name>` must fail with the
   same symptom as the test262 case. If it doesn't, you don't understand the
   bug yet; keep refining the test before touching production code.
3. **Fix** — make the minimal change to production code so the unit test
   passes.
4. **Verify** — re-run the unit test, the module's full test suite, then the
   relevant test262 stage. Run `cargo fmt`/`cargo clippy` before declaring done.
5. **Leave the test in** — regression coverage stays in the tree. A fix
   without a committed test is not done.

No `println!`-driven archaeology. No speculative rewrites. No "let me try
this" patches. If you can't express the bug as a unit test, you don't have
enough understanding to change the code.

test262 output is a signal for *what* to test, not a substitute for the test
itself — the conformance run lives in `tests/test262.rs` only and is never
edited; the reproductions live next to the code under `src/.../mod tests`.

## Architecture

```
crates/quench-runtime/src/
├── parser.rs      # OXC → internal AST
├── lower/         # AST lowering
├── ast.rs         # internal AST
├── interpreter.rs # eval entry points
├── eval/          # tree-walking evaluator
├── env.rs         # lexical environments
├── value/         # Value, Object, Function, NativeFunction, JsError
├── builtins/      # native builtins (Object, Array, Map, Symbol, Promise, ...)
├── context/       # Context, globals, CURRENT_CONTEXT
└── test262/      # runner.rs, harness/, metadata.rs
```

## Conventions

- **Builtins throw `JsError`**, never panic. Use
  `crate::value::error::create_js_error_with_type` and
  `crate::value::set_thrown_value`.
- **Minimal diffs** — match surrounding style, no opportunistic refactors.
- **Symbols**: `Value::Symbol` payload is raw `desc\0id` string; used as property key directly.
- **Boxed primitives**: stored via `builtins::object::set_boxed_value` as `_value` property.
- **Function strictness** captured at definition, never inherited from call site.
  Class bodies are always strict.
- **Accessor properties**: use `Object::define_accessor`; `GetterStorage.func` takes precedence.
- **`CURRENT_CONTEXT`** (context/mod.rs): `thread_local` raw pointer set for the duration of eval.
- New builtins wired in `builtins/mod.rs::register_builtins` — Symbol before Map/Set, Number before Date.
