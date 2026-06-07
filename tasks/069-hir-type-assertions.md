# Task 069: Add `as` / `satisfies` / Non-null to HIR + Parser

**Priority:** P2-Medium
**Phase:** 7 — HIR & Parser Expansion
**Depends on:** 068
**Status:** COMPLETED

## Problem

TypeScript `as`, `satisfies`, and non-null assertions (`!`) are not represented in HIR.

## Work

### 1. Add HIR variants ✅

```rust
TypeAssertion {
    expr: Box<Expr>,
    type_: Box<Type>,
}
Satisfies {
    expr: Box<Expr>,
    type_: Box<Type>,
}
NonNull {
    expr: Box<Expr>,
}
```

Added to:
- `crates/runts-hir/src/expr.rs`
- `src/transpile/hir/expr.rs` (re-export)

### 2. Update parser ✅

Handle `Expression::TSAsExpression`, `Expression::TSSatisfiesExpression`, `Expression::TSNonNullExpression`.

All three are type-erased - they just emit the inner expression.

### 3. Update codegen ✅

All are erased — emit inner expression only:

```rust
E::TypeAssertion { expr, .. } => self.gen_expr(expr),
E::Satisfies { expr, .. } => self.gen_expr(expr),
E::NonNull { expr } => self.gen_expr(expr),
```

### 4. Tests added ✅

- `spec_types/codegen_verification.rs`: Tests that all three variants emit inner expression
- `spec_types/roundtrip_integration.rs`: Tests that parsing produces correct HIR

### 5. Example added ✅

- `examples/ink-type-assertions/`: Demonstrates `as`, `satisfies`, and `!`

## Acceptance Criteria

- [x] `expr as Type`, `expr satisfies Type`, `expr!` all parse into HIR
- [x] Codegen erases all three (emits inner expression only)
- [x] Parser and codegen tests added
