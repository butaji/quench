# Task 23: Improve diagnostics and error messages

## Goal

Make every Quench error — parser, lowering, runtime, or bridge — as clear and helpful as possible. A user should always know what went wrong, where, and what to do next.

## Status: IN PROGRESS

### What was done

1. **Added typed error types** to `crates/quench-runtime/src/value/error.rs`:
   - `JsErrorType::Error` - generic error
   - `JsErrorType::ReferenceError` - undefined variables
   - `JsErrorType::TypeError` - type mismatches
   - `JsErrorType::SyntaxError` - syntax errors
   - `JsErrorType::RangeError` - out of range
   - `JsErrorType::URIError` - URI errors
   - `JsErrorType::InternalError` - internal errors

2. **Added typed error constructors**:
   - `JsError::reference_error(msg)` - creates a ReferenceError
   - `JsError::type_error(msg)` - creates a TypeError
   - `JsError::range_error(msg)` - creates a RangeError
   - `JsError::syntax_error(msg)` - creates a SyntaxError
   - `JsError::with_type(type, msg)` - creates an error with any type

3. **Added error inspection methods**:
   - `err.error_type()` - returns the error type
   - `err.message()` - returns the error message
   - Display format shows `TypeName: message` for typed errors

4. **Added unit tests** for error types

### Remaining work

- [ ] Add source spans to HIR nodes for better location reporting
- [ ] Implement runtime stack traces
- [ ] Add pretty-print formatting for errors with source snippets
- [ ] Update LowerError with source location support
- [ ] Audit lowering for silent drops

## Files Modified

- `crates/quench-runtime/src/value/error.rs` - added typed errors
- `crates/quench-runtime/tests/runtime_tests.rs` - added tests

## Timeout note

- All test commands must run with a timeout to avoid hangs from interpreter bugs or infinite loops.
- Use the `scripts/run_tests.sh` wrapper (if available) or prefix commands with `timeout 120` / `gtimeout 120`.
- In CI, set per-test and job-level timeouts (e.g., 5 minutes per test suite, 30 minutes per job).


## Verification

```bash
cargo test -p quench-runtime -- test_error
```
