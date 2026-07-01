# Debug Statement Removal - quench-runtime

## Summary
Removed all debug `eprintln!("DEBUG ...")` statements from 7 source files in `crates/quench-runtime/src/`. Total removed: ~50 debug statements.

## Files Modified

### 1. `crates/quench-runtime/src/interpreter/binary_ops.rs`
- Removed `eprintln!("DEBUG eval_binary_op: ...")` from `eval_binary_op()`
- Removed `eprintln!("DEBUG BinaryOp::Eq: ...")` from `BinaryOp::Eq` case
- Removed all `eprintln!("DEBUG abstract_eq: ...")` from `abstract_eq()` and `object_to_primitive()`

### 2. `crates/quench-runtime/src/interpreter/eval_stmt/loops.rs`
- Removed 4 debug statements from `call_iterator_and_collect()`
- Fixed unused variable warning: `Err(e)` → `Err(_e)`

### 3. `crates/quench-runtime/src/interpreter/eval_expr/main.rs`
- Removed `eprintln!("DEBUG eval_expression: Binary ...")` from `eval_expression_impl()`

### 4. `crates/quench-runtime/src/interpreter/eval_expr/helpers_obj.rs`
- Removed 7 debug statements from `eval_identifier()` and `eval_binary_expr()`

### 5. `crates/quench-runtime/src/interpreter/eval_expr/helpers_call.rs`
- Removed all 12 debug statements from `eval_new_expression()` and `create_new_object()`

### 6. `crates/quench-runtime/src/builtins/numbers.rs`
- Removed 1 debug statement from `take_pending_number_value()`

### 7. `crates/quench-runtime/src/builtins/global.rs`
- Removed 3 debug statements from `take_pending_boolean_value()` and `Boolean.valueOf`

## Verification

### Test Results
```
cargo test -p quench-runtime
  105 passed; 0 failed; 2 ignored (runtime_tests)
  6 passed (benchmarks)
  5 passed (compiler_cases)
  24 passed; 8 ignored (conformance)
  4 passed (evaluation)
  45 passed (runtime_tests integration)
  Total: 189 tests, all pass
```

### Compilation
```
cargo check
  Finished `dev` profile — 1 warning (fixed: unused variable `e` → `_e`)
  No errors
```

### Remaining Debug Statements
Only debug statements remaining are in `context/tests.rs` (test file, not modified per instructions).
