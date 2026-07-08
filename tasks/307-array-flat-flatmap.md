# Task 307: Implement Array.flat and Array.flatMap

## Status: COMPLETED

## Gap

- Array.flat existed but was not registered on Array.prototype
- Array.flatMap was missing entirely

## Fix

1. **Registered Array.flat** in `setup_prototype_methods()` in `array.rs`

2. **Implemented Array.flatMap** as `proto_flat_map()`:
   - Like `map`, but flattens the result by one level
   - For each element, calls the callback and adds results to the output array
   - If callback returns an array, the array's elements are added (not the array itself)

3. **Fixed array literal prototype chain**: Modified `eval_array()` in `eval_expr.rs` to use `get_array_prototype()` so array literals have access to Array.prototype methods.

## Files

- `crates/quench-runtime/src/builtins/array.rs` - Added `proto_flat_map()` and registered in `setup_prototype_methods()`
- `crates/quench-runtime/src/interpreter/eval_expr.rs` - Modified `eval_array()` to use Array prototype

## Tests

All Array.flat/flatMap tests pass:
- test_array_flat
- test_array_flat_map

## Notes

- The flat implementation uses recursive `flatten_array()` helper
- flatMap uses `call_callback()` to properly invoke callbacks with (element, index, array) args
- `ObjectKind::Array` check is used to determine if flatMap result should be flattened

## Verification

Run the conformance harnesses and confirm the relevant built-ins subset passes at
100% with zero spec skips:

```bash
cargo test -p quench-runtime --test test262 -- --ignored --nocapture
cargo test -p quench-runtime --test conformance -- --test-threads=1
# Inspect target/test262_report.json and target/conformance_report.json:
# built-ins/Array subset shows 100% pass and 0 spec skips.
```

Run unit regression tests:

```bash
cargo test -p quench-runtime
```
