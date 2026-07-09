> Sub-task of 290: complete expression syntax gaps.

# Task 290f: Implement unary plus

## Status: COMPLETED

## Implementation

The unary `+` operator is implemented via:
- `crates/quench-runtime/src/ast.rs` - `UnaryOp::Plus` enum variant
- `crates/quench-runtime/src/lower/helpers.rs` - `lower_unary_op` maps `swc::UnaryOp::Plus => UnaryOp::Plus`
- `crates/quench-runtime/src/eval/operators.rs` - `eval_unary_op` handles `UnaryOp::Plus` via `to_number(val)`

## Verification

All tests pass:
- `+"42"` → `42` ✓
- `+'5'` → `5` ✓
- `+true` → `1` ✓
- `+false` → `0` ✓
- `+undefined` → `NaN` ✓

Tests in `crates/quench-runtime/tests/runtime_issues.rs`:
- `test_unary_plus_number`
- `test_unary_plus_string_to_number`
- `test_unary_plus_boolean_true`
- `test_unary_plus_boolean_false`
- `test_unary_plus_undefined`

## Files

- `crates/quench-runtime/src/ast.rs`
- `crates/quench-runtime/src/lower/helpers.rs`
- `crates/quench-runtime/src/eval/operators.rs`

## Tests

```bash
cargo test -p quench-runtime --test runtime_issues
# All unary_plus tests pass
```
