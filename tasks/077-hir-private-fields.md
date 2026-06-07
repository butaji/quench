# Task 077: Add Private Fields `#field` to HIR + Parser + Codegen

**Priority:** P2-Medium
**Phase:** 7 — HIR & Parser Expansion
**Depends on:** 076

## Problem

Private fields (`#field`) in classes are not represented in HIR.

## Work

### 1. Add HIR variant

Private fields can reuse `PrivateMember` expr and add a new `ClassMember` variant:
```rust
PrivateField {
    name: String,
}
```

### 2. Update parser

Handle `PrivateIdentifier` in class properties and `PrivateFieldExpression` in member access.

### 3. Update codegen

Map `#field` to Rust private struct field:
```rust
struct ClassName {
    field: Type,  // private by default in Rust
}
```

## Acceptance Criteria

- [ ] `#field` declarations in classes parse into HIR
- [ ] `#field` access (`obj.#field`) parses into HIR
- [ ] Codegen produces compilable Rust with private fields
- [ ] `ink-static-private` example (Task 060) passes parity harness