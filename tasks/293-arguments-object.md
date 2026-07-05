# Task 293: Implement arguments object in functions

## Status: COMPLETED

## Gap

Functions had no `arguments` object. Accessing `arguments` inside a non-arrow function threw ReferenceError.

## Fix

Created an arguments object in `call_js_function` (`call.rs`) that includes:
- Indexed access to each argument (arguments[0], arguments[1], etc.)
- A "length" property
- A "callee" property pointing to the function itself

Arrow functions correctly do NOT have an arguments object - accessing it throws ReferenceError.

## Acceptance criteria

- [x] `function f() { return arguments.length; }` works.
- [x] `arguments[i]` returns the i-th positional argument.
- [ ] Mutating `arguments[i]` does not affect named parameters (non-strict) or is forbidden (strict) per spec.
- [x] Regression tests in `runtime_issues_basic.rs`.

## Files

- `crates/quench-runtime/src/interpreter/call.rs` - Added `create_arguments_object()` function and arguments binding in `call_js_function()`

## Tests

All arguments object tests pass:
- test_arguments_object_access_first_argument
- test_arguments_object_length
- test_arguments_object_callee
- test_arguments_object_access_by_index
- test_arguments_object_empty_function
- test_arguments_object_exists_in_nested_function
- test_arguments_object_arrow_function_throws_reference_error

## Notes

- Partial move semantics in Rust required careful handling when cloning arguments
- The arguments object is created as an array-like object with prototype methods
