# Task 01: Fix parser and lowering bugs in quench-runtime

## Goal

Make the swc-based parser/lowering pipeline robust enough to ingest `src/runtime.js` and TSX/JSX source directly, without any pre-compilation step, and produce a clean HIR that is suitable for both interpretation and future AOT compilation.

## Pareto & reuse note

- Prefer existing crates, the Rust standard library, and OS features over custom code.
- Follow the 80/20 rule: implement the subset that unblocks the targeted examples/conformance tests first.
- Defer edge cases, but document them in this task or spawn a follow-up task so they are not lost.

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
- A snippet using spread, getters/setters, and ES module syntax parses and lowers to HIR.
- The HIR is documented well enough that a future AOT backend can pattern-match on it without re-parsing source.

## Verification

```bash
cargo check -p quench-runtime
cargo test -p quench-runtime
```
