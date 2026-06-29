# Task 01: Fix parser and lowering bugs in quench-runtime

## Goal

Make the swc-based parser/lowering pipeline robust enough to ingest `src/runtime.js` and compiled TSX output without compile-time or lowering errors.

## Files

- `crates/quench-runtime/src/swc_parse.rs`
- `crates/quench-runtime/src/lower.rs`
- `crates/quench-runtime/src/ast.rs` (only if new AST nodes are needed)

## Done

- Computed member property access (`obj[expr]`) lowers correctly.
- Template literal expressions are interleaved into binary `+` trees.
- Optional chaining for member and call (`obj?.prop`, `obj?.[expr]`, `obj?.()`) is lowered.
- `for...of` and `for...in` loops (including destructuring loop heads) are lowered.
- Rest parameters are extracted from function/arrow signatures.
- `??`, `in`, and `instanceof` binary operators are lowered.

## Still to do

- Add module/script fallback: `parse_swc` uses `parse_script` only; compiled TSX may arrive as an ES module.
- Preserve real getter/setter metadata in object literals so the interpreter can invoke them.
- Add lowering for object/array spread syntax (`{...obj}`, `[...arr]`).

## Steps

1. Add `parse_module` (or auto-detect) and make `Context::eval` fall back to module parsing when script parsing fails.
2. Add AST nodes for getter/setter properties and lower `get prop() {}` / `set prop(v) {}` correctly.
3. Add AST nodes for spread expressions and lower them in array/object literals and function calls.

## Boundaries

- Work only in `crates/quench-runtime/src/`.
- Do not touch `src/bridge/`, `src/ink/`, `src/render/`, `src/compiler/`.
- `examples/` are immutable.

## Acceptance criteria

- `cargo check -p quench-runtime` and `cargo test -p quench-runtime` pass.
- `ctx.eval(include_str!("../../../src/runtime.js"))` parses without lowering errors.
- A snippet using spread, getters/setters, and ES module syntax parses and lowers.

## Verification

```bash
cargo check -p quench-runtime
cargo test -p quench-runtime
```
