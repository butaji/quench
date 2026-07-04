# Task 283: Install String.prototype once instead of per-access closures

## Status: PENDING

## Problem

Accessing `"foo".length` or `"foo".indexOf(...)` constructs a fresh `NativeFunction` closure capturing `s_clone` and `prop_name_clone` every time. This allocates on every string property access.

## Fix

Create one `String.prototype` object at runtime initialization, install shared `NativeFunction`s there, and set it as the prototype of string wrapper objects (or handle string boxing centrally).

## Acceptance criteria

- [ ] `String.prototype` is built once during `init_builtins`.
- [ ] String property access reuses the shared prototype methods.
- [ ] Regression test: repeated `"x".length` does not allocate new closures.
- [ ] JS scenario tests for string methods.

## Files

- `crates/quench-runtime/src/interpreter.rs`
- `crates/quench-runtime/src/value.rs`
- `crates/quench-runtime/src/builtins/string.rs` (if exists)

## Verification

```bash
cargo test -p quench-runtime string_prototype_shared
cargo test -p quench-runtime scenarios::string
```
