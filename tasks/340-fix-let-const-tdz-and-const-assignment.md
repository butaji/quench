> **Exact fix for TDZ and const assignment checks.**

# Task 340: Fix let/const TDZ and const assignment TypeError

## Problem

`let` and `const` bindings do not enforce the temporal dead zone, and assignments to `const` bindings are silently accepted.

Failing tests:
```javascript
let y = 1; { y; let y = 2; }   // should throw TDZ
let z; z; let z = 1;           // should throw TDZ
const x = 1; x = 2;            // should throw TypeError
```

## Exact implementation

Edit `crates/quench-runtime/src/env.rs` and `crates/quench-runtime/src/interpreter.rs`:

1. Extend `Binding` (or equivalent) in `env.rs` to store:
   ```rust
   enum VarKind { Var, Let, Const }
   struct Binding {
       value: Value,
       kind: VarKind,
       initialized: bool,
   }
   ```
2. In `declare_var`, set `initialized = false` for `Let`/`Const`, and `true` for `Var` (because `var` hoisting leaves it `undefined` but accessible).
3. Add `fn set_initialized(&mut self, name: &str)` to mark a binding initialized after its initializer evaluates.
4. In every read/write helper (`get_var`, `set_var`, `assign_to`), if the binding's `initialized == false`, return `ReferenceError: Cannot access 'X' before initialization`.
5. In `assign_to`, if the binding's `kind == Const`, return `TypeError: Assignment to constant variable.`.
6. In `eval_var_decl` for `let`/`const`, evaluate the initializer, write the value, then call `set_initialized`.

## Verification

```bash
cargo test -p quench-runtime --test var_hoisting_tdz test_let_tdz_access_before_init test_tdz_block_scope test_tdz_shadowing_inner_let test_const_assignment_throws_type_error
```

Expected: all pass.

## Targets

- **Suite:** `test262`
- **Batch:** 2
- **Target subset:** `tests/test262/test/language/statements/let/` and `tests/test262/test/language/statements/const/`
- **Blocked by:** 338, 339
- **Exit criteria:** All TDZ and const-assignment tests in `var_hoisting_tdz.rs` pass, and the test262 let/const subsets improve.
