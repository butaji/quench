# Task 299: Implement optional chaining (?.) operator

## Status: COMPLETED

## Summary

Implemented optional chaining operator (`?.`) for the quench runtime. This syntax allows safe property access and function calls on null or undefined values.

## Implementation

### AST Changes (crates/quench-runtime/src/ast.rs)
- Added `OptChain(OptChainExpr)` variant to `Expression` enum
- Added `OptChainExpr` enum with three variants:
  - `Member` - property access: `obj?.prop`
  - `ComputedMember` - computed access: `obj?.[expr]`
  - `Call` - function call: `fn?.()`

### Parser Lowering (crates/quench-runtime/src/lower/expr.rs)
- Added `lower_opt_chain()` function to handle `swc::OptChainExpr`
- Added `lower_opt_chain_base()` to handle `swc::OptChainBase` variants
- Added helper functions `lower_opt_chain_object()` and `lower_opt_chain_call_callee()`

### Interpreter (crates/quench-runtime/src/interpreter/expr.rs)
- Added `is_nullish()` helper function
- Added `eval_opt_chain()` to evaluate optional chain expressions
- Handles short-circuit to `undefined` when base is null/undefined

### Member Module (crates/quench-runtime/src/interpreter/member.rs)
- Added `eval_member_property()` helper for optional chaining computed access

## Tests Added

1. `scenario_optional_chaining_basic` - Basic property access
2. `scenario_optional_chaining_property_access` - Non-null base property access
3. `scenario_optional_chaining_null_shortcircuit` - Short-circuit on null
4. `scenario_optional_chaining_undefined_shortcircuit` - Short-circuit on undefined
5. `scenario_optional_chaining_computed` - Computed property access
6. `scenario_optional_chaining_call` - Function call on valid base
7. `scenario_optional_chaining_call_null` - Function call returns undefined on null

All 7 tests pass.

## Validation

```bash
cargo test -p quench-runtime scenario_optional_chaining
# Result: 7 passed; 0 failed

cargo check
# Result: Success

timeout 60 cargo run -- examples/counter.js
# Result: Success

timeout 60 cargo run -- examples/use-bridge.tsx --prop theme=dark
# Result: Success

timeout 60 cargo run -- examples/animations.tsx
# Result: Success
```

## Remaining Gaps

- Nested optional chaining with mixed forms (`obj?.foo()?.[bar]`) - not yet tested
- Template literal optional chains (`` obj?.`template` ``) - not supported by swc parser in this version
