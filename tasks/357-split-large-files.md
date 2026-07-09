# Task 357: Split large files to meet 500-line limit

## Status

**OPEN** - High Priority

## Problem

The quench-runtime contains multiple files that exceed the 500-line limit:

| File | Lines |
|------|-------|
| `interpreter.rs` | 1644 |
| `shadow.rs` | 1633 |
| `stack_machine.rs` | 1603 |
| `lower.rs` | 1195 |
| `value.rs` | 901 |
| `test262/runner.rs` | 682 |
| `lib.rs` | 632 |

## Exit Criteria

- All `*.rs` files in `crates/quench-runtime/src/` under 500 lines
- All `*.rs` files in `crates/quench-runtime/src/` with functions under 40 lines
- All `*.rs` files in `crates/quench-runtime/src/` with complexity under 10
- No `#[allow(...)]` exemptions
- All existing tests pass

## Strategy

### 1. value.rs (901 lines) → value/types.rs + value/ops.rs + value.rs

Split into:
- `value/types.rs`: Value, Object, ObjectKind, Getter, Setter, ValueFunction, NativeFunction, NativeConstructor
- `value/ops.rs`: to_js_string, to_bool, to_number, strict_eq, loose_eq, to_primitive
- `value.rs`: Re-exports only (~50 lines)

### 2. lower.rs (1195 lines) → lower/stmt.rs + lower/expr.rs + lower.rs

Split into:
- `lower/stmt.rs`: Statement lowering functions
- `lower/expr.rs`: Expression lowering functions
- `lower.rs`: Entry points and helpers (~200 lines)

### 3. interpreter.rs (1644 lines) → interpreter/*.rs

Split into:
- `interpreter/control.rs`: ControlFlow, set/take_control_flow
- `interpreter/depth.rs`: Depth tracking
- `interpreter/this.rs`: This binding
- `interpreter/eval_expr.rs`: eval_expression (~500 lines)
- `interpreter/eval_stmt.rs`: eval_statement, eval_statements (~300 lines)
- `interpreter/call.rs`: call_value, call_value_with_this (~200 lines)
- `interpreter.rs`: Module re-exports (~100 lines)

### 4. shadow.rs (1633 lines) → shadow/*.rs

Split into:
- `shadow/types.rs`: ShadowObject, ShadowNode, ShadowFrame, etc.
- `shadow/vm.rs`: ShadowVm implementation (~1200 lines)
- `shadow.rs`: Re-exports (~50 lines)

### 5. stack_machine.rs (1603 lines) → stack_machine/*.rs

Split into:
- `stack_machine/machine.rs`: Machine struct and impl
- `stack_machine/ops.rs`: Operation handlers
- `stack_machine.rs`: Entry points (~200 lines)

## Verification

```bash
# Check line counts
find crates/quench-runtime/src -name "*.rs" -exec wc -l {} \; | sort -rn

# Run all tests
cargo test -p quench-runtime

# Verify examples
cargo run -- examples/counter.js
cargo run -- examples/use-bridge.tsx --prop theme=dark
cargo run -- examples/animations.tsx
```
