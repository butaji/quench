> **Exact fix for constructors returning the expression value instead of `this`.**

# Task 341: Fix constructor to return `this` when no explicit object return

## Status: IN PROGRESS

## Progress

Background process landed an initial fix in `interpreter.rs` and `lib.rs` that makes `test_constructor_returns_this_not_expression_value` pass. A remaining bug affects property assignment on the constructed object (`this.props = props || {}` leaves `this.props` as `undefined` when `props` is undefined). The fix below must be completed before closing this task.

## Problem

When a constructor has no explicit `return`, the expression value of the body is returned instead of the newly created `this` object.

Failing test:
```javascript
new Boolean(false) // returns false; should return a Boolean object
```

## Exact implementation

Edit `crates/quench-runtime/src/interpreter.rs` and `crates/quench-runtime/src/lib.rs`:

1. In the `[[Construct]]` path, before executing the function body:
   - Create a new ordinary object.
   - Set its `[[Prototype]]` to `callee.prototype`.
   - Bind it as `this` in the new function environment.
2. In `crates/quench-runtime/src/ast.rs`, add `Statement::has_explicit_return()` so the constructor path can distinguish an explicit `return` from an implicit last expression.
3. Execute the function body, capturing any explicit `return` value.
4. After execution:
   - If the body contained an explicit `return` and the returned value is an object, return it.
   - Otherwise, return the `this` binding.
5. Verify property assignment on the constructed object works (`this.props = props || {}` must leave `this.props` as `{}` when `props` is undefined).
4. Remove any code that returns the implicit last expression value in constructor mode.

## Verification

```bash
cargo test -p quench-runtime --test var_hoisting_tdz test_constructor_returns_this_not_expression_value
```

Expected: passes.

## Targets

- **Suite:** `test262`
- **Batch:** 2
- **Target subset:** `tests/test262/test/language/functions/constructor/`
- **Blocked by:** 338
- **Exit criteria:** `test_constructor_returns_this_not_expression_value` passes and constructor-return behavior matches ECMAScript.
