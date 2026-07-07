> Sub-task of 290: complete expression syntax gaps.

# Task 290c: Implement the spread operator

## Status: PENDING

## Gap

Spread syntax in arrays (`[...arr]`), objects (`{...obj}`), and function calls (`fn(...args)`) is not supported.

## Fix

- Lower `swc::Expr::Spread` in array literals, object literals, and call arguments.
- In arrays: iterate the spread operand and append elements.
- In objects: copy enumerable own properties from the spread operand.
- In calls: expand arguments before binding parameters.

## Acceptance criteria

- [ ] `[1, ...[2, 3], 4]` evaluates to `[1, 2, 3, 4]`.
- [ ] `{ a: 1, ...{ b: 2 } }` evaluates to `{ a: 1, b: 2 }`.
- [ ] `Math.max(...[1, 2, 3])` returns `3`.
- [ ] Regression tests and JS scenario tests for each context.

## Files

- `crates/quench-runtime/src/lower.rs`
- `crates/quench-runtime/src/ast.rs`
- `crates/quench-runtime/src/interpreter/eval_expr.rs`
- `crates/quench-runtime/src/interpreter/mod.rs`

## Tests unblocked

- test262 `language/expressions/array/spread-*`, `language/expressions/call/`
- TypeScript spread failures
