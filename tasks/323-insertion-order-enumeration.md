> New low-effort/high-impact win from code review.

# Task 323: Make object property enumeration insertion-ordered

## Status: COMPLETED

## Problem

Properties are stored in `HashMap`, whose iteration order is unspecified. ECMA-262 requires `Object.keys`, `Object.entries`, `Object.values`, and `for-in` to follow insertion order.

## Fix

Replaced `HashMap<String, Value>` for own properties with `IndexMap` in `crates/quench-runtime/src/value.rs`. Numeric keys are still sorted first per spec.

## Acceptance criteria

- [x] `Object.keys({a:1, b:2, c:3})` returns `["a","b","c"]`.
- [x] Deleting and re-adding a key moves it to the end.
- [x] Regression tests and fixtures added (`value::tests::test_object_keys_insertion_order`, `test_object_keys_delete_readd_moves_to_end`, `test_object_keys_numeric_first`).

## Verification

```bash
cargo test -p quench-runtime --lib value::tests::test_object_keys_insertion_order
cargo test -p quench-runtime --lib value::tests::test_object_keys_delete_readd_moves_to_end
cargo test -p quench-runtime --lib value::tests::test_object_keys_numeric_first
```

Expected: all pass.

## Files

- `crates/quench-runtime/src/value.rs`

## Effort / impact

- Effort: low–medium
- Impact: high
