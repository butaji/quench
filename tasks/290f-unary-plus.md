> Sub-task of 290: complete expression syntax gaps.

# Task 290f: Implement unary plus

## Status: PENDING

## Gap

The unary `+` operator is not implemented. `+x` fails or is parsed as binary plus.

## Fix

- Lower `swc::UnaryOp::Plus` to an AST unary-plus expression.
- Evaluate by coercing the operand to a number via `ToNumber` semantics.

## Acceptance criteria

- [ ] `+"42"` evaluates to `42`.
- [ ] `+true` evaluates to `1`.
- [ ] `+null` evaluates to `0`.
- [ ] `+""` evaluates to `0`.
- [ ] Regression test and JS scenario test.

## Files

- `crates/quench-runtime/src/lower.rs`
- `crates/quench-runtime/src/ast.rs`
- `crates/quench-runtime/src/interpreter/eval_expr.rs`
- `crates/quench-runtime/src/interpreter/ops.rs`

## Tests unblocked

- test262 `language/expressions/unary-plus/`
- TypeScript unary operator failures
