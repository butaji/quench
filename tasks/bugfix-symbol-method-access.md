# Bug Fix: Symbol Method Access via eval_callee_with_this

## Summary

Fixed a bug where calling methods on `Symbol` values failed with "Value is not a function".

## Root Cause

The `eval_callee_with_this` function in `crates/quench-runtime/src/interpreter/call.rs` did not handle `Value::Symbol` when evaluating member access expressions on Symbol values.

When evaluating `Symbol('x').toString()`:
1. The callee is `Symbol('x').toString` (a Member expression)
2. `eval_callee_with_this` evaluates the object (`Symbol('x')`) to get `Value::Symbol("x.N")`
3. The function had no case for `Value::Symbol`, so it fell through to `_ => Value::Undefined`
4. Attempting to call `Value::Undefined` as a function caused "Value is not a function"

## Fix

Added a case for `Value::Symbol` in `eval_callee_with_this` that calls `eval_symbol_member` to properly resolve Symbol prototype methods:

```rust
Value::Symbol(s) => {
    crate::interpreter::eval_expr::helpers_call::eval_symbol_member(s, &prop_name, &Rc::clone(env))?
}
```

Also made `eval_symbol_member` public so it can be called from `call.rs`.

## Files Changed

- `crates/quench-runtime/src/interpreter/call.rs` - Added Symbol case to `eval_callee_with_this`
- `crates/quench-runtime/src/interpreter/eval_expr/helpers_call.rs` - Made `eval_symbol_member` public

## Test Added

Added `test_symbol_method_access_regression` in `crates/quench-runtime/tests/runtime_tests.rs` to verify:
- `Symbol('x').toString()` works
- `Symbol('y').valueOf()` works  
- `Symbol('test-desc').description` works
- Chained Symbol method calls work

## Verification

```bash
cargo test -p quench-runtime test_symbol_method_access_regression
# Result: ok

cargo test -p quench-runtime
# Result: 40 passed; 0 failed
```
