# Task 53: Fix remaining runtime test failures

## Goal

Fix the 10 runtime test failures that were blocking full test suite green status.

## Status: COMPLETED

All 10 tests are now fixed. Runtime tests: **92 passed** (was 82).

## What was fixed

### 1. `test_do_while_works` ✅
- **Problem**: `do...while` statement was not supported in lowering.
- **Fix**: Added `DoWhile` variant to `Statement` enum in `ast.rs`, implemented `lower_do_while_stmt` in `lower/stmt.rs`, and added `eval_do_while` in `interpreter/eval_stmt/mod.rs`.
- **File**: `crates/quench-runtime/src/ast.rs`, `crates/quench-runtime/src/lower/stmt.rs`, `crates/quench-runtime/src/interpreter/eval_stmt/mod.rs`

### 2. `test_object_to_primitive_coercion` ✅
- **Problem**: `abstract_eq` in `binary_ops.rs` checked `boolean == any` before `object == primitive`, causing `new Boolean(true) == true` to return false.
- **Fix**: Reordered comparison branches to check object-to-primitive BEFORE boolean-to-number. Also fixed `boolean_fn` to return `this` for Boolean constructor calls.
- **File**: `crates/quench-runtime/src/interpreter/binary_ops.rs`, `crates/quench-runtime/src/builtins/global.rs`

### 3. `test_template_literal_edge_cases` ✅
- **Problem**: Tagged template literals returned `Err("Tagged templates not supported")`.
- **Fix**: Added `TaggedTemplate` variant to `Expression` enum, implemented `lower_tagged_template` in `lower/expr.rs`, and added `eval_tagged_template` in `interpreter/eval_expr/helpers_call.rs`.
- **File**: `crates/quench-runtime/src/ast.rs`, `crates/quench-runtime/src/lower/expr.rs`, `crates/quench-runtime/src/interpreter/eval_expr/helpers_call.rs`, `crates/quench-runtime/src/interpreter/eval_expr/main.rs`

### 4. `test_export_named_error` ✅
- **Problem**: `ctx.eval("export const x = 1")` returned `Ok` instead of `Err`.
- **Fix**: Modified `lower_export_decl` in `lower/stmt.rs` to return errors for all unsupported ES module export declarations (`export const`, `export function`, `export class`, etc.).
- **File**: `crates/quench-runtime/src/lower/stmt.rs`

### 5. JSX lowering tests (6 tests) ✅
- **Problem**: JSX tests used `ctx.eval()` (ES mode) but source had TypeScript type annotations and JSX syntax.
- **Fix**: Added `eval_tsx()` and `parse_tsx()` methods to `Context` that use `tsx: true` parsing mode. Updated all 6 JSX tests to use `ctx.eval_tsx()`.
- **Tests fixed**:
  - `test_jsx_basic_element`
  - `test_jsx_self_closing_element`
  - `test_jsx_with_attributes`
  - `test_jsx_fragment`
  - `test_jsx_nested_elements`
  - `test_jsx_expr_container`
- **File**: `crates/quench-runtime/src/context/mod.rs`, `crates/quench-runtime/tests/runtime_tests.rs`

### 6. Compile error fix ✅
- **Problem**: Lifetime error in `helpers_call.rs` - `is_number_constructor` closure returned a reference to borrowed data.
- **Fix**: Rewrote the check using explicit if-let instead of chained `and_then`/`map`.
- **File**: `crates/quench-runtime/src/interpreter/eval_expr/helpers_call.rs`

## Verification

```bash
cargo test -p quench-runtime --test runtime_tests
# → 92 passed; 0 failed; 1 ignored

cargo test
# → 34 passed (bridge) + 5 passed (parity) + 19 passed (conformance) + 92 passed (runtime)

cargo run --release -- examples/simple.js
cargo run --release -- examples/counter.js
cargo run --release -- examples/use-bridge.tsx --prop theme=dark
cargo run --release -- examples/animations.tsx
# → All examples work correctly
```

## Test count summary

| Suite | Before | After |
|-------|--------|-------|
| Runtime tests | 82 passed | 92 passed |
| Main crate tests | 34 passed | 34 passed |
| Parity tests | 5 passed | 5 passed |
| Conformance tests | 19 passed | 19 passed |

## Remaining work

See Task 54 for the next set of priorities.
