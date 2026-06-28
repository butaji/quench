# Task 01: Create quench-runtime crate and define AST/value model

## Goal

Create the dedicated `quench-runtime` crate, add its dependencies, and define the core AST and value types that the custom execution engine builds on. Parsing is out of scope — swc will handle that in Task 02.

## Files

- Modify: `Cargo.toml` (root) — add workspace and `quench-runtime` path dependency
- Create: `crates/quench-runtime/Cargo.toml`
- Create: `crates/quench-runtime/src/lib.rs`
- Create: `crates/quench-runtime/src/ast.rs`
- Create: `crates/quench-runtime/src/value.rs`

## Steps

1. In root `Cargo.toml`:
   - Add `[workspace]` with `members = [".", "crates/quench-runtime"]`.
   - Add `quench-runtime = { path = "crates/quench-runtime" }` under `[dependencies]`.
   - Remove `rquickjs` later in Task 07; leave it for now.
2. Create `crates/quench-runtime/Cargo.toml` with dependencies:
   - `swc_common`, `swc_ecma_parser`, `swc_ecma_ast`
   - `string_cache` and `lasso`
   - `indexmap`
   - `num-bigint`, `rust_decimal`
   - `bitflags`
   - `hashbrown`
   - `thiserror`, `tracing`, `serde_json` (for built-ins/JSON)
3. Audit `src/runtime.js`, `src/compiler/mod.rs` output, and `examples/*.js` for required language features. The minimum set is:
   - literals: number, string, boolean, null, undefined
   - objects, arrays, member access (`obj.prop`, `obj[prop]`), optional not required
   - function declarations, function expressions, arrow functions (used sparingly)
   - calls, `new` not required
   - `var`/`let`/`const`, assignment, compound assignment
   - `if`/`else`, `while`, `for`, `return`, `break`/`continue`
   - `try`/`catch` optional; runtime.js currently catches in timer dispatch, so either support it or rewrite that block
   - operators: `+ - * / % === !== == != < > <= >= && || ! typeof ++ --`
   - `JSON.stringify`, `JSON.parse`, `Object`, `Array`, `Map`, `Set`, `Math`, `Date`, `console`
4. Define `crates/quench-runtime/src/ast.rs` with an enum that covers every construct above.
5. Define `crates/quench-runtime/src/value.rs` with `Value` enum including object/array/function/native function/error variants, plus `Display`/`Debug` helpers.
6. Design `value.rs` to use the support crates:
   - Intern all string keys/identifiers with `lasso`/`string_cache`.
   - Store object properties in `indexmap` for enumeration order and `hashbrown` for fast lookup.
   - Represent numbers as `f64` with optional `num-bigint::BigInt` and `rust_decimal::Decimal` support.
   - Use `bitflags!` for object shape flags (`extensible`, `callable`, `constructor`, etc.).
   - Support `Value::Object`, `Value::Array`, `Value::String`, `Value::Number`, `Value::BigInt`, `Value::Bool`, `Value::Null`, `Value::Undefined`, `Value::Function`, `Value::NativeFunction`, `Value::Error`.
7. In `crates/quench-runtime/src/lib.rs` expose:
   - `pub struct Runtime; pub struct Context;`
   - `Context::eval(source: &str) -> Result<Value>`
   - `Context::set_global(name: &str, value: Value)`
   - `Context::get_global(name: &str) -> Option<Value>`
   - `Context::call_function(name: &str, args: Vec<Value>) -> Result<Value>`
   - `Context::register_native_function(name: &str, f: Box<dyn Fn(&[Value]) -> Result<Value>>)` for host functions.
8. Add `use quench_runtime::{Context, Runtime, Value};` to `src/main.rs` so the crate is linked.

## Boundaries

- Do not modify any existing source files outside of root `Cargo.toml` and adding the `use` statement to `src/main.rs`.
- Do not change `src/bridge/`, `src/ink/`, `src/render/`, `src/compiler/`, or `src/cli.rs`.
- `quench-runtime` must not depend on any `quench` internals.

## Acceptance criteria

- `cargo check -p quench-runtime` passes.
- `ast.rs` and `value.rs` contain no `TODO` or stub variants for the required feature set.
- A short unit test in `crates/quench-runtime/src/lib.rs` proves values can be constructed and compared.

## Verification

```bash
cargo check -p quench-runtime
```
