# Task 080: Add Decorators to HIR + Parser (Type Erasure)

**Priority:** P3-Low
**Phase:** 7 — HIR & Parser Expansion
**Depends on:** 079

## Problem

Decorators (`@decorator`) are parsed by oxc but not represented in HIR. HIR `ClassDecl` already has a `decorators: Vec<Decorator>` field but it's likely unused.

## Work

### 1. Verify HIR support

Check if `Decorator` struct in HIR is populated by the parser.

### 2. Update parser

Handle `Decorator` nodes in class/method declarations.

### 3. Update codegen

For v1, decorators are erased (no runtime code). For compile path, this is acceptable since decorators transform at compile time.

## Acceptance Criteria

- [ ] Decorators parse into HIR without producing `Invalid`
- [ ] Codegen erases decorators (no runtime impact)
- [ ] Parser tests added
