# Task 291: typeof undeclared returns "undefined"

## Status: PENDING

## Gap

`typeof x` throws `ReferenceError` when `x` is undeclared. ECMA-262 requires it to return `"undefined"`.

## Fix

Special-case `typeof` operand resolution to use a soft lookup that does not error on missing bindings.

## Acceptance criteria

- [ ] `typeof notDeclared === "undefined"` is true without throwing.
- [ ] `typeof declaredButUndefined === "undefined"` still true.
- [ ] Regression test and JS scenario test added.

## Files

- `crates/quench-runtime/src/interpreter/expressions.rs` or `ops.rs`

## Tests unblocked

- `typeof` tests
- TS `typeofOperator` / `voidOperator` failures
