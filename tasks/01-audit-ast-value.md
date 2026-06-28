# Task 01: Audit JS surface and define AST/value model

## Goal

Decide exactly which JS features the interpreter must support, add the required dependencies, and create the core AST and value types that everything else builds on.

## Files

- Modify: `Cargo.toml`
- Create: `src/js_runtime/mod.rs`
- Create: `src/js_runtime/ast.rs`
- Create: `src/js_runtime/value.rs`

## Steps

1. Add dependencies to `Cargo.toml` under `[dependencies]`:
   - `swc_common`, `swc_ecma_parser`, `swc_ecma_ast`
   - `string_cache` and `lasso`
   - `indexmap`
   - `num-bigint`, `rust_decimal`
   - `bitflags`
   - `hashbrown`
2. Audit `src/runtime.js`, `src/compiler/mod.rs` output, and `examples/*.js` for required language features. The minimum set is:
   - literals: number, string, boolean, null, undefined
   - objects, arrays, member access (`obj.prop`, `obj[prop]`), optional not required
   - function declarations, function expressions, arrow functions (used sparingly)
   - calls, `new` not required
   - `var`/`let`/`const`, assignment, compound assignment
   - `if`/`else`, `while`, `for`, `return`, `break`/`continue`
   - `try`/`catch` optional; runtime.js currently catches in timer dispatch, so either support it or rewrite that block
   - operators: `+ - * / % === !== == != < > <= >= && || ! typeof ++ --`
   - `JSON.stringify`, `JSON.parse`, `Object`, `Array`, `Map`, `Set`, `Math`, `Date`, `console`
2. Define `src/js_runtime/ast.rs` with an enum that covers every construct above.
3. Define `src/js_runtime/value.rs` with `Value` enum including object/array/function/native function/error variants, plus `Display`/`Debug` helpers.
4. In `src/js_runtime/mod.rs` expose:
   - `pub struct Runtime; pub struct Context;`
   - `Context::eval(source: &str) -> Result<Value>`
   - `Context::set_global(name: &str, value: Value)`
   - `Context::get_global(name: &str) -> Option<Value>`
   - `Context::call_function(name: &str, args: Vec<Value>) -> Result<Value>`
5. Design `src/js_runtime/value.rs` to use the support crates:
   - Intern all string keys/identifiers with `lasso`/`string_cache`.
   - Store object properties in `indexmap` for enumeration order and `hashbrown` for fast lookup.
   - Represent numbers as `f64` with optional `num-bigint::BigInt` and `rust_decimal::Decimal` support.
   - Use `bitflags!` for object shape flags (`extensible`, `callable`, `constructor`, etc.).
   - Support `Value::Object`, `Value::Array`, `Value::String`, `Value::Number`, `Value::BigInt`, `Value::Bool`, `Value::Null`, `Value::Undefined`, `Value::Function`, `Value::NativeFunction`, `Value::Error`.
6. Add `mod js_runtime;` to `src/main.rs` so the module compiles.

## Boundaries

- Do not modify any existing source files outside of creating `src/js_runtime/` files and adding `mod js_runtime;` to `src/main.rs`.
- Do not change `src/bridge/`, `src/ink/`, `src/render/`, `src/compiler/`, or `src/cli.rs`.

## Acceptance criteria

- `cargo check` passes with the new module.
- `ast.rs` and `value.rs` contain no `TODO` or stub variants for the required feature set.
- A short unit test in `src/js_runtime/mod.rs` proves values can be constructed and compared.

## Verification

```bash
cargo check
```
