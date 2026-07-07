> Sub-task of 290: complete expression syntax gaps.

# Task 290a: Implement template literals

## Status: PENDING

## Gap

Template literals (`` `hello ${name}` ``) are not lowered or evaluated. They show up as top TypeScript expression failures.

## Fix

- Lower `swc::Tpl` AST nodes into an AST expression that concatenates cooked strings with evaluated embedded expressions.
- Support tagged templates (future/optional for this task; basic untagged literals are required).

## Acceptance criteria

- [ ] `` `a${1+2}b` `` evaluates to `"a3b"`.
- [ ] `` `x${`y${1}z`}w` `` evaluates nested templates correctly.
- [ ] Embedded expressions are evaluated in the current scope.
- [ ] Regression test and JS scenario test.

## Files

- `crates/quench-runtime/src/lower.rs`
- `crates/quench-runtime/src/ast.rs`
- `crates/quench-runtime/src/interpreter/eval_expr.rs`

## Tests unblocked

- test262 `language/expressions/template-literal/`
- TypeScript template literal failures
