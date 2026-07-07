> Sub-task of 290: complete expression syntax gaps.

# Task 290d: Implement nullish coalescing (`??`)

## Status: PENDING

## Gap

The nullish coalescing operator (`a ?? b`) is not lowered. It is needed for many modern JS/TS snippets.

## Fix

- Lower `swc::BinOp::NullishCoalescing` to an AST node or desugar to a conditional.
- Evaluate the left operand; if it is `null` or `undefined`, evaluate and return the right operand.

## Acceptance criteria

- [ ] `null ?? 1` evaluates to `1`.
- [ ] `undefined ?? 2` evaluates to `2`.
- [ ] `0 ?? 3` evaluates to `0`.
- [ ] `false ?? 4` evaluates to `false`.
- [ ] Regression test and JS scenario test.

## Files

- `crates/quench-runtime/src/lower.rs`
- `crates/quench-runtime/src/ast.rs`
- `crates/quench-runtime/src/interpreter/eval_expr.rs`

## Tests unblocked

- test262 `language/expressions/coalesce/`
- TypeScript nullish coalescing failures
