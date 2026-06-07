# Task 073: Add Dynamic Import `import()` to HIR + Parser + Codegen

**Priority:** P2-Medium
**Phase:** 7 — HIR & Parser Expansion
**Depends on:** 072

## Problem

Dynamic imports (`import('./module')`) are not represented in HIR.

## Work

### 1. Add HIR variant

```rust
ImportDynamic {
    source: Box<Expr>,
}
```

### 2. Update parser

Handle `Expression::ImportExpression`.

### 3. Update codegen

For v1, emit compile error: "dynamic imports not supported in compile path".

## Acceptance Criteria

- [ ] `import()` and `import.meta` parse into HIR (not `Invalid`)
- [ ] Codegen handles both (even if placeholder/error)
- [ ] Parser tests added
- [ ] `ink-dynamic-import` example (Task 059) passes in dev path
