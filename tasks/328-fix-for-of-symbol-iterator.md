> New low-effort/high-impact win from code review.

# Task 328: Fix for-of Symbol.iterator lookup

## Status: PENDING

## Problem

`for-of` looks up a property literally named `"Symbol"` instead of the well-known `Symbol.iterator` symbol. Custom iterables do not work.

## Fix

Resolve `Symbol.iterator` from the global symbol registry or from the object’s own symbol-keyed properties, and call it.

## Acceptance criteria

- [ ] `for (const x of [1,2,3])` still works.
- [ ] `for (const x of customIterable)` works when `customIterable[Symbol.iterator]` is defined.
- [ ] Regression test and fixture added.

## Files

- `crates/quench-runtime/src/interpreter.rs` (`get_iterator`)

## Effort / impact

- Effort: low
- Impact: medium
