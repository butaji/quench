> **Exact fix for `typeof this` returning `"undefined"` at script level.**

# Task 345: Bind script-level `this` to the global object

## Problem

At the top level of a script, `this` is unbound and `typeof this` returns `"undefined"`.

Failing test:
```javascript
typeof this // "undefined"; should be "object"
```

## Exact implementation

Edit `crates/quench-runtime/src/interpreter.rs` `eval_program`:

1. For `Program::Script(statements)`:
   - Before calling `hoist_functions` or evaluating statements, set the current `this` binding to the global object.
   - Pass this binding through to statement/expression evaluation.
2. For `Program::Module(statements)`:
   - Bind `this` to `Value::Undefined`.
3. Ensure `eval_unary_op` for `UnaryOp::Typeof` uses the resolved `this` binding.

## Verification

```bash
cargo test -p quench-runtime --test var_hoisting_tdz test_typeof_this_returns_object
```

Expected: passes.

## Targets

- **Suite:** `test262`
- **Batch:** 2
- **Target subset:** `tests/test262/test/language/expressions/this/`
- **Blocked by:** 338
- **Exit criteria:** `test_typeof_this_returns_object` passes and script-level `typeof this` is `"object"`.
