# Task 071: Add Private Fields `#field` to HIR + Parser + Codegen

**Priority:** P2-Medium
**Phase:** 7 — HIR & Parser Expansion
**Depends on:** 070

## Problem

Private fields (`#field`) in classes are not represented in HIR.

## Work

### 1. Add HIR support

Reuse `PrivateMember` expr and add `ClassMember::PrivateField`.

### 2. Update parser

Handle `PrivateIdentifier` in class properties and `PrivateFieldExpression` in member access.

### 3. Update codegen

Map `#field` to Rust private struct field.

## Acceptance Criteria

- [ ] `#field` declarations and access parse into HIR
- [ ] Codegen produces compilable Rust with private fields
- [ ] `ink-static-private` example (Task 056) passes with 100% parity
