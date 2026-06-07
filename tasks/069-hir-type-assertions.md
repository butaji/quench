# Task 069: Add `as` / `satisfies` / Non-null to HIR + Parser

**Priority:** P2-Medium
**Phase:** 7 — HIR & Parser Expansion
**Depends on:** 068

## Problem

TypeScript `as`, `satisfies`, and non-null assertions (`!`) are not represented in HIR.

## Work

### 1. Add HIR variants

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

### 2. Update parser

Handle `Expression::TSAsExpression`, `Expression::TSSatisfiesExpression`, `Expression::TSNonNullExpression`.

### 3. Update codegen

All are erased — emit inner expression only.

## Acceptance Criteria

- [ ] `expr as Type`, `expr satisfies Type`, `expr!` all parse into HIR
- [ ] Codegen erases all three (emits inner expression only)
- [ ] Parser and codegen tests added
