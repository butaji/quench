# Task 141: Arrow function use strict validation

## Status: PENDING

## Gap

Strict mode is not enforced consistently. Arrow functions and function bodies with `"use strict"` directives do not validate parameter names or body contents against strict-mode restrictions, causing `onlyStrict` test262 failures.

## Fix

- Detect `"use strict"` directives in function and script bodies.
- Reject duplicate parameter names and `with` statements in strict mode.
- Propagate strict mode to nested scopes correctly.

## Acceptance criteria

- [ ] `function f(a, a) { "use strict"; }` throws a SyntaxError.
- [ ] Arrow functions inherit strict mode from enclosing scope.
- [ ] `with` statement is rejected in strict mode.
- [ ] Focused regression tests for strict-mode validation.

## Files

- `crates/quench-runtime/src/lower.rs`
- `crates/quench-runtime/src/ast.rs`
- `crates/quench-runtime/src/interpreter/*.rs`

## Tests unblocked

- test262 `language/directive-prologue/`
- test262 `language/function-code/`
- `onlyStrict` tagged tests
