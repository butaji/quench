> Sub-task of 290: complete expression syntax gaps.

# Task 290g: Implement `for-of` loops

## Status: PENDING

## Gap

`for-of` loops are not supported. Iteration over arrays, strings, and iterables fails.

## Fix

- Lower `swc::Stmt::ForOf` to an AST `Statement::ForOf`.
- Evaluate the right-hand side, get its iterator (`Symbol.iterator`), and loop until done.
- Support `for (const x of iterable)` and `for (let x of iterable)`.

## Acceptance criteria

- [ ] `for (const x of [1, 2, 3])` iterates `1`, `2`, `3`.
- [ ] `for (const ch of "ab")` iterates `"a"`, `"b"`.
- [ ] Custom iterables with `Symbol.iterator` work.
- [ ] Regression test and JS scenario test.

## Files

- `crates/quench-runtime/src/lower.rs`
- `crates/quench-runtime/src/ast.rs`
- `crates/quench-runtime/src/interpreter/eval_stmt.rs`

## Tests unblocked

- test262 `language/statements/for-of/`
- TypeScript `for-of` failures
