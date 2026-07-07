> Sub-task of 290: complete expression syntax gaps.

# Task 290e: Implement the `delete` operator

## Status: PENDING

## Gap

The `delete` operator is not implemented. Expressions like `delete obj.prop` fail or return wrong values.

## Fix

- Lower `swc::UnaryOp::Delete` to an AST `Expression::Delete`.
- Implement semantics:
  - `delete obj.prop` removes the property and returns `true`/`false` per spec.
  - `delete obj[prop]` removes computed property.
  - `delete identifier` returns `false` for declared variables and true for unresolvable references in non-strict mode.
  - Throw in strict mode when deleting an identifier.

## Acceptance criteria

- [ ] `delete obj.prop` removes the property and returns `true`.
- [ ] `delete obj.nonExistent` returns `true`.
- [ ] Strict-mode `delete x` throws `SyntaxError` (or `TypeError` at runtime).
- [ ] Regression test and JS scenario test.

## Files

- `crates/quench-runtime/src/lower.rs`
- `crates/quench-runtime/src/ast.rs`
- `crates/quench-runtime/src/interpreter/eval_expr.rs`
- `crates/quench-runtime/src/interpreter/eval_expr.rs`

## Tests unblocked

- test262 `language/expressions/delete/`
- TypeScript `delete` operator failures
