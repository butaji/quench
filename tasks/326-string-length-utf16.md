> New low-effort/high-impact win from code review.

# Task 326: String.prototype.length uses UTF-16 code units

## Status: PENDING

## Problem

`String.prototype.length` uses `s.len()` (UTF-8 bytes). JS requires UTF-16 code units, so `"🙂".length` returns 4 instead of 2.

## Fix

Use `s.encode_utf16().count()` for spec-accurate length in both `builtins/string.rs` and the inline string-path branch in `interpreter.rs`.

## Acceptance criteria

- [ ] `"🙂".length` returns 2.
- [ ] `"abc".length` returns 3.
- [ ] Regression test and fixture added.

## Files

- `crates/quench-runtime/src/builtins/string.rs`
- `crates/quench-runtime/src/interpreter.rs`

## Effort / impact

- Effort: trivial
- Impact: medium
