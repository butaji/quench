# Task 01: Fix parser and lowering bugs in quench-runtime

## Goal

Make the swc-based parser/lowering pipeline robust enough to ingest `src/runtime.js` and compiled TSX output without compile-time or lowering errors.

## Files

- `crates/quench-runtime/src/swc_parse.rs`
- `crates/quench-runtime/src/lower/` (now split into submodules)
- `crates/quench-runtime/src/ast.rs`

## Done ✓

- Computed member property access (`obj[expr]`) lowers correctly.
- Template literal expressions are interleaved into binary `+` trees.
- `for...of` and `for...in` loops (including destructuring loop heads) are lowered.
- `??`, `in`, and `instanceof` binary operators are lowered.
- Getter/setter properties (`get prop() {}`, `set prop(v) {}`) are lowered correctly.
- Object and array spread (`{...obj}`, `[...arr]`) is lowered.
- **Module/script fallback**: `parse_swc` now tries module syntax first if `import`/`export` is present.
- **lower.rs split into submodules**: `lower/mod.rs`, `lower/decl.rs`, `lower/expr.rs`, `lower/stmt.rs`, `lower/helpers.rs`, `lower/patterns.rs`

## Still missing / caveats

- **Optional chaining** (`obj?.prop`, `obj?.[expr]`, `obj?.()`) is currently rejected by the lowerer.
- **Destructuring assignment** (`[a, b] = arr`, `({x} = obj)`) is not lowered.
- **Destructuring function/arrow parameters** are renamed to `"arg"`; patterns are dropped.
- **Rest parameters in arrow functions** are ignored.
- **Class expressions** are rejected.
- **`delete` operator** and unary `+` are rejected.

## Acceptance criteria

- `cargo check -p quench-runtime` and `cargo test -p quench-runtime` pass.
- `ctx.eval(include_str!("../../../src/runtime.js"))` parses without lowering errors.
- A snippet using spread, getters/setters, and ES module syntax parses and lowers.

## Verification

```bash
cargo check -p quench-runtime
cargo test -p quench-runtime
```
