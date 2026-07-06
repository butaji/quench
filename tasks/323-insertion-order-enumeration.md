> New low-effort/high-impact win from code review.

# Task 323: Make object property enumeration insertion-ordered

## Status: PENDING

## Problem

Properties are stored in `HashMap`, whose iteration order is unspecified. ECMA-262 requires `Object.keys`, `Object.entries`, `Object.values`, and `for-in` to follow insertion order.

## Fix

Replace `HashMap<String, Value>` for own properties with `IndexMap` (or maintain an insertion-order list alongside the map).

## Acceptance criteria

- [ ] `Object.keys({a:1, b:2, c:3})` returns `["a","b","c"]`.
- [ ] Deleting and re-adding a key moves it to the end.
- [ ] Regression tests and fixtures added.

## Files

- `crates/quench-runtime/src/value.rs`

## Effort / impact

- Effort: low–medium
- Impact: high
