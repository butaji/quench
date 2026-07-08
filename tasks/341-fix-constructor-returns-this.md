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

Edit `crates/quench-runtime/src/interpreter/call.rs` and `crates/quench-runtime/src/value.rs`:

1. In the `[[Construct]]` path, before executing the function body:
   - Create a new ordinary object.
   - Set its `[[Prototype]]` to `callee.prototype`.
   - Bind it as `this` in the new function environment.
2. Execute the function body, capturing any explicit `return` value.
3. After execution:
   - If the returned value is an object, return it.
   - Otherwise, return the `this` binding.
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
