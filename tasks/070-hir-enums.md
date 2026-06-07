# Task 070: Add Enum Declarations to HIR + Parser + Codegen

**Priority:** P2-Medium
**Phase:** 7 — HIR & Parser Expansion
**Depends on:** 069
**Status:** COMPLETED

## Problem

TypeScript `enum` declarations are not represented in HIR.

## Work

### 1. Add HIR variants ✅

Added to `crates/runts-hir/src/base.rs`:
```rust
EnumDecl {
    name: String,
    members: Vec<EnumMember>,
    is_const: bool,
}
EnumMember {
    key: String,
    value: Option<EnumValue>,
}
EnumValue {
    Number(f64),
    String(String),
}
```

Also added `Stmt::Enum(EnumDecl)` to Stmt enum.

### 2. Update parser ✅

Handle `Statement::TSEnumDeclaration` in `src/transpile/parser/stmt.rs`.

### 3. Update codegen ✅

Added `gen_enum()` in `src/transpile/hir/quote_codegen_stmts.inc`.

### 4. Tests added ✅

- `spec_types/roundtrip_integration.rs`: Tests for numeric, string, and const enums
- `spec_types/codegen_verification.rs`: Tests for enum codegen output

## Acceptance Criteria

- [x] Numeric enums parse into HIR and codegen produces compilable Rust
- [x] String enums parse into HIR and codegen produces compilable Rust
- [x] Const enums are inlined (no runtime code)
- [x] Parser and codegen tests added
- [x] `ink-enum-types` example (Task 066) passes with 100% parity
