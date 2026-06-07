# Task 076: Add Enum Declarations to HIR + Parser + Codegen

**Priority:** P2-Medium
**Phase:** 7 — HIR & Parser Expansion
**Depends on:** 075

## Problem

TypeScript `enum` declarations are not represented in HIR.

## Work

### 1. Add HIR variant

```rust
Enum {
    id: String,
    members: Vec<EnumMember>,
    is_const: bool,
}
```

### 2. Update parser

Handle `Statement::TSEnumDeclaration`.

### 3. Update codegen

- **Numeric enums:** `#[repr(i32)] enum Name { A = 1, B }`
- **String enums:** Module with `const` strings + reverse lookup
- **Const enums:** Inline values at usage sites

## Acceptance Criteria

- [ ] Numeric enums parse into HIR and codegen produces compilable Rust
- [ ] String enums parse into HIR and codegen produces compilable Rust
- [ ] Const enums are inlined (no runtime code)
- [ ] Parser and codegen tests added
- [ ] `ink-enum-types` example (Task 072) passes parity harness