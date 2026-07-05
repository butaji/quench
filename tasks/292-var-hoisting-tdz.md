# Task 292: Implement var hoisting and let/const TDZ

## Status: COMPLETED

## Implementation

- **var hoisting**: `var` declarations are now hoisted to function scope (or script scope at top level). The variable is declared as `DeclaredOnly` (undefined value) at scope entry, then initialized when the declaration statement is evaluated.

- **let/const TDZ**: `let` and `const` declarations are pre-declared in TDZ (Temporal Dead Zone) state at block/function entry. Accessing a variable in TDZ throws `ReferenceError: Cannot access 'X' before initialization`.

- **const assignment**: Assigning to a `const` variable throws `TypeError: Assignment to constant variable`.

## Files modified

- `crates/quench-runtime/src/env.rs` - Added `VarState::TDZ` variant, updated `declare_var` to handle TDZ for let/const, updated `initialize_declared` to work across scopes, added `is_tdz` and `declare_var_in_current` methods
- `crates/quench-runtime/src/interpreter/eval_stmt.rs` - Updated `eval_block` to pre-declare let/const in TDZ, updated `eval_var_decl` to properly initialize TDZ variables
- `crates/quench-runtime/src/interpreter/call.rs` - Added `collect_var_names_from_stmts`, `collect_let_const_from_stmts` and helpers to hoist var declarations and pre-declare let/const in TDZ at function call time
- `crates/quench-runtime/src/interpreter/runtime.rs` - Updated `hoist_declarations` to also handle let/const declarations

## Acceptance criteria

- [x] `console.log(x); var x = 1;` logs `undefined`.
- [x] `x; let x = 1;` throws `ReferenceError` (TDZ).
- [x] `const x = 1; x = 2;` throws `TypeError`.
- [x] Regression tests and JS scenario tests for hoisting and TDZ.

## Tests

All 11 tests in `crates/quench-runtime/tests/var_hoisting_tdz.rs` pass:
- `test_var_hoisting_basic`
- `test_var_hoisting_logs_undefined`
- `test_var_function_scope`
- `test_for_loop_var_hoisting`
- `test_let_tdz_with_reference_error`
- `test_let_no_tdz_after_init`
- `test_let_tdz_access_before_init`
- `test_const_tdz_access_before_init`
- `test_const_assignment_throws_type_error`
- `test_const_can_read_after_init`
- `test_nested_tdz`

## Tests unblocked

- Large swath of variable-scope tests
- Many "x is not defined" TS failures
