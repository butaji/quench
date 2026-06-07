# Task 079: Add Dynamic Import `import()` to HIR + Parser + Codegen

**Priority:** P2-Medium
**Phase:** 7 — HIR & Parser Expansion
**Depends on:** 078

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

For compile path, dynamic imports are tricky. Options:
- Emit compile error: "dynamic imports not supported in compile path"
- For known static sources, inline the module
- For v1: emit descriptive error

## Acceptance Criteria

- [ ] `import('./module')` parses into HIR (not `Invalid`)
- [ ] `import.meta` parses into HIR (not `Invalid`)
- [ ] Codegen handles both (even if just emitting a placeholder/error)
- [ ] Parser tests added
- [ ] `ink-dynamic-import` example (Task 063) passes parity harness in dev path