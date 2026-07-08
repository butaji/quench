> **Exact fix for var declarations not being hoisted inside function bodies.**

# Task 339: Fix var hoisting inside function scope

## Status: COMPLETED

## Verification

Implementation landed by the background process; `var_hoisting_tdz` function-scope `var` tests pass.

## Problem

`var` declarations inside a function body are not hoisted, so accessing the variable before its declaration throws `ReferenceError` instead of returning `undefined`.

Failing test:
```javascript
function f() { console.log(x); var x = 1; }
f(); // ReferenceError: x is not defined
```

## Exact implementation

Edit `crates/quench-runtime/src/interpreter.rs` and `crates/quench-runtime/src/env.rs`:

1. Add a helper `collect_var_names(stmts: &[Statement]) -> Vec<String>` that walks the immediate statement list and returns the names of every `Statement::VarDeclaration { name, .. }`.
2. When a user function is called and a new function-scope environment is created, call `collect_var_names` on the function body.
3. For each collected name, call `env.declare_var(name, Value::Undefined, VarKind::Var)` before executing any statements.
4. Ensure `eval_var_decl` later assigns the initializer without re-declaring the binding.

Do not hoist `let`/`const` here; that is Task 340.

## Verification

```bash
cargo test -p quench-runtime --test var_hoisting_tdz test_var_function_scope test_var_hoisting_logs_undefined
```

Expected: both pass.

## Targets

- **Suite:** `test262`
- **Batch:** 2
- **Target subset:** `tests/test262/test/language/statements/variable/`
- **Blocked by:** 338
- **Exit criteria:** `var_hoisting_tdz.rs` `test_var_function_scope` and `test_var_hoisting_logs_undefined` pass, and the test262 variable-statement subset improves.
