# Deferred: Strict Build-Time Linting Rules

## Status

The file-length limit (500 lines) is **ENFORCED** by `build.rs`.
Function-length and complexity limits are **DEFERRED** as documented below.

## Rules Status

| Rule | Status | Notes |
|------|--------|-------|
| File length: 500 lines | **ENFORCED** | Build fails on violations |
| Function length: 40 lines | **DEFERRED** | ~46 violations, requires architectural changes |
| Complexity: 10 | **DEFERRED** | ~20 violations in complex match arms |
| No `#[allow(...)]` exemptions | **PARTIAL** | No new exemptions allowed |

## 1. 500 Lines/File Limit - ENFORCED

The linter in `build.rs` checks file lengths and panics on violations.
As of 2026-07-09, no files in `crates/quench-runtime/src/` exceed 500 lines.

## 2. 40 Lines/Function Limit - DEFERRED

### Violations (~46 functions)

These violations require significant refactoring to fix. They are acceptable
for now as documented here.

| File | Count | Notes |
|------|-------|-------|
| `stack_machine/*.rs` | ~15 | Core interpreter, splitting requires pub(crate) |
| `eval/*.rs` | ~12 | Complex AST traversal patterns |
| `builtins/**/*.rs` | ~8 | Promise and other builtins |
| `lower/**/*.rs` | ~6 | AST lowering logic |
| `test262/**/*.rs` | ~5 | Test harness (acceptable) |

### Acceptance Criteria

- Functions that handle complex state machines are allowed
- Helper functions should remain under 40 lines when possible
- New code should not add violations

## 3. Complexity 10 Limit - DEFERRED

### Violations (~20 functions)

Complex match arms in interpreter and lowering code exceed complexity 10.
These are acceptable for JS semantics correctness.

| File | Count | Notes |
|------|-------|-------|
| `eval/expression.rs` | 2 | eval_expression match arms |
| `eval/statement.rs` | 1 | statement evaluation |
| `eval/class.rs` | 1 | class evaluation |
| `stack_machine/*.rs` | ~4 | interpreter loops |
| `hir.rs` | 1 | HIR lowering |
| `test262/**/*.rs` | ~10 | Test harness (acceptable) |

## 4. No `#[allow(...)]` Exemptions

**Status:** PARTIALLY ACCEPTABLE

- No `#[allow]` attributes for lint rules
- Clippy warnings addressed incrementally
- Existing warnings documented in `cargo clippy` output

## Deferred Action Plan

When refactoring for other reasons, consider:
1. Split large functions into helper functions
2. Extract complex match arms into separate functions
3. Use guard clauses to reduce nesting

## Verification

```bash
cargo build  # Only file-length violations cause failure
```
