# Task 01: Fix parser and lowering bugs in quench-runtime

## Goal

Make the swc-based parser/lowering pipeline robust enough to ingest `src/runtime.js` and compiled TSX output without compile-time or lowering errors.

## Files

- `crates/quench-runtime/src/swc_parse.rs`
- `crates/quench-runtime/src/lower.rs`
- `crates/quench-runtime/src/ast.rs` (only if new AST nodes are needed)

## Current issues

- `parse_swc` uses `parse_script` only; compiled TSX may arrive as an ES module, so support both `parse_script` and `parse_module` (auto-detect or expose `parse_module`).
- `lower_member_prop` for computed properties returns an empty `PropertyKey::String` instead of lowering the inner expression.
- `lower_template_literal` joins the static `quasis` and drops embedded expressions entirely.
- `lower_expr` rejects `OptChain` (`?.`) with an error.
- `lower_expr` handles `Tpl` but drops embedded expressions.
- `lower_decl` / arrow/function params map non-identifier and rest params to `"arg"`, dropping rest args.
- `lower_stmt` returns `None` for `ForIn` and `ForOf`, silently deleting loops.
- `lower_bin_op` does not handle `NullishCoalescing` (`??`).
- Getters/setters are lowered as plain functions named `"get"` / `"set"` instead of first-class getter/setter metadata.

## Steps

1. Add `parse_module` (or auto-detect) and make `Context::eval` fall back to module parsing when script parsing fails.
2. Fix `lower_member_prop` to lower computed expressions into `PropertyKey::Computed`.
3. Fix `lower_template_literal` to interleave `quasis` and lowered expressions into a binary `+` expression.
4. Lower optional chaining (`a?.b`) to a conditional expression that checks `a != null` before accessing `a.b`.
5. Add rest-parameter support: represent rest as a special param marker or expand `function(cb, ...args)` so `args` is bound to `Array.prototype.slice.call(arguments, n)`.
6. Add AST support and lowering for `for...of` and `for...in` loops.
7. Add `BinaryOp::NullishCoalescing` and handle it in the interpreter (return left if not null/undefined, else right).
8. Preserve getter/setter metadata in object literals so the interpreter can invoke them on property access.

## Boundaries

- Work only in `crates/quench-runtime/src/`.
- Do not touch `src/bridge/`, `src/ink/`, `src/render/`, `src/compiler/`.
- `examples/` are immutable.

## Acceptance criteria

- `cargo check -p quench-runtime` and `cargo test -p quench-runtime` pass.
- `ctx.eval(include_str!("../../../src/runtime.js"))` parses without lowering errors.
- A snippet using optional chaining, template literals, rest params, `for...of`, `for...in`, and `??` parses and lowers.

## Verification

```bash
cargo check -p quench-runtime
cargo test -p quench-runtime
```
