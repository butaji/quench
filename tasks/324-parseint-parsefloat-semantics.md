> New low-effort/high-impact win from code review.

# Task 324: Implement spec-correct parseInt and parseFloat

## Status: PENDING

## Problem

`parseInt`/`parseFloat` use Rust's `str::parse`, requiring the whole string to be valid. They ignore leading/trailing whitespace, radix handling, hex/octal/binary prefixes, and early termination (e.g., `parseInt("12px")` should be 12, not NaN).

## Fix

Implement the ECMAScript `parseInt`/`parseFloat` algorithms: trim whitespace, parse optional radix prefix, stop at first invalid character, return NaN when appropriate.

## Acceptance criteria

- [ ] `parseInt("  12px  ")` returns 12.
- [ ] `parseInt("0xFF")` returns 255.
- [ ] `parseInt("xyz")` returns NaN.
- [ ] `parseFloat("3.14abc")` returns 3.14.
- [ ] Regression tests and fixtures added.

## Files

- `crates/quench-runtime/src/builtins/date.rs` (`register_global_functions`) or a new globals module

## Effort / impact

- Effort: low–medium
- Impact: high
