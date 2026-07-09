# Task 105: Implement Error.prototype default methods

## Status: COMPLETED

## Goal

Add `Error.prototype.toString` and other default methods expected by built-in/Error tests.

## Acceptance criteria

- [x] `new Error('test').toString()` returns "Error: test".
- [x] `new Error().toString()` returns "Error".
- [x] `new TypeError('invalid').toString()` returns "TypeError: invalid".
- [x] `new Error('').toString()` returns "Error" (empty message).
- [x] Regression tests added in `runtime_issues.rs`.

## Implementation

- Modified `crates/quench-runtime/src/builtins/error.rs`
- `create_error_proto` now creates a proper toString that:
  1. Gets `this` via `get_native_this()`
  2. Throws TypeError if `this` is not an object
  3. Gets `this.name` (defaults to error type name)
  4. Gets `this.message` (defaults to "" if undefined)
  5. Formats: name + ": " + message (with empty-string shortcuts)

## Tests

- `test_error_to_string_with_message` - verifies "Error: test"
- `test_error_to_string_without_message` - verifies "Error"
- `test_error_to_string_empty_message` - verifies "Error" for empty message

## Verification

```bash
cargo test -p quench-runtime --test runtime_issues test_error_to_string
```

All 3 tests pass. Clippy clean. Lint compliant (99 lines, <40 lines/function).
Commit: 420120b feat(builtins): implement Error.prototype.toString
