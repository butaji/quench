# Task 284: Avoid cloning array elements in builtin methods

## Status: PENDING

## Problem

Array prototype methods call `get_this_array()` which does `arr.elements.clone()`, and mutating methods replace the whole vector via `set_this_elements`. This makes every element access O(n).

## Fix

Pass a mutable borrow of the array object / elements vector into callbacks and operate on `&mut Vec<Value>` directly. Avoid copying the vector unless absolutely necessary.

## Acceptance criteria

- [ ] `get_this_array` returns a borrow, not a clone.
- [ ] Mutating methods update the array in place.
- [ ] Regression test: large array operations do not OOM or degrade quadratically.
- [ ] JS scenario tests for map/filter/push/pop.

## Files

- `crates/quench-runtime/src/builtins/array.rs`

## Verification

```bash
cargo test -p quench-runtime array_no_clone
cargo test -p quench-runtime scenarios::array
```
