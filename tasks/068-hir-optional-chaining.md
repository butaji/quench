# Task 068: Add Optional Chaining `?.` to HIR + Parser + Codegen

**Priority:** P1-High
**Phase:** 7 — HIR & Parser Expansion
**Depends on:** 061

## Problem

Optional chaining (`obj?.prop`, `arr?.[0]`, `fn?.()`) is **not represented in HIR**. Parser falls through to `Expr::Invalid`.

## Work

### 1. Add HIR variants

```rust
OptionalMember {
    obj: Box<Expr>,
    property: Box<Expr>,
    computed: bool,
}
OptionalCall {
    callee: Box<Expr>,
    arguments: Vec<Expr>,
}
```

### 2. Update parser

Handle `Expression::ChainExpression` in `src/transpile/parser/expr.rs`.

### 3. Update codegen

Desugar to conditional access:
```rust
// obj?.prop
{
    let __obj = obj;
    if __obj.is_null() || __obj.is_undefined() {
        Value::Undefined
    } else {
        __obj.prop
    }
}
```

## Acceptance Criteria

- [ ] `obj?.prop`, `obj?.[key]`, `fn?.()` all parse into HIR (not `Invalid`)
- [ ] Codegen produces compilable Rust for all three forms
- [ ] Parser and codegen tests added
- [ ] `ink-nullish-optional` example (Task 049) passes with 100% parity
