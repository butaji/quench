> New low-effort/high-impact win from code review.

# Task 320: Fix object storage for array indices

## Status: PENDING

## Problem

Numeric keys are stored in both `elements` and `properties`. `delete` only removes from `properties`, and `own_keys` never includes element indices. This breaks `Object.keys([1,2,3])`, `delete arr[0]`, and the distinction between own array indices and named properties.

## Fix

- In `Object::set`, put numeric keys only in `elements`; do not mirror them in `properties`.
- Update `Object::delete` to remove from `elements` when the key is numeric.
- Update `own_keys` to include element indices for arrays.

## Acceptance criteria

- [ ] `Object.keys([1,2,3])` returns `["0","1","2"]`.
- [ ] `delete arr[0]` removes the element and returns true.
- [ ] Named properties on arrays still work.
- [ ] Regression test and fixture added.

## Files

- `crates/quench-runtime/src/value.rs`

## Effort / impact

- Effort: low
- Impact: high
